//! **Stage 3 of the new rendering chain — fit range.**
//!
//! Fit the scene's dynamic range into the display's. The industry term is *tone
//! mapping*; "tone" there means brightness levels, not colour. One operator, with the
//! display's **peak** as its argument — which is what makes SDR and HDR the same
//! function rather than two renderers:
//!
//! ```text
//! Y′ = r(Y) · (1 + (P − 1) · s(Y))            per pixel, on ACEScg luminance
//! r  = mid-grey-preserving extended reinhard at W = 2^headroom_stops
//! s  = smoothstep in stops, 0 at diffuse white → 1 at W
//! ```
//!
//! All three channels are scaled by `Y′/Y`, so hue and the neutral axis are kept and
//! chroma is left to the look and to fit gamut. At `Y ≤ 0` — a saturated colour a
//! wide-gamut linear space can hold — the scale is its limit at black, the mid-grey
//! gain, so it stays continuous where luminance crosses zero.
//!
//! - **`P = 1` is exactly reinhard.** The lift multiplies by `1 + 0·s = 1`, so on the
//!   SDR branch reinhard's per-pixel arithmetic is transcendental-free; its one libm
//!   call is the white point, `2^headroom_stops` (`exp2`), exact at whole stops.
//!   **The dark end is not**: it calls `log2` and `exp2` per frame and per pixel below
//!   mid-grey (see below), so where it acts, output below mid-grey may differ by a ULP
//!   across targets. From mid-grey up, and where it leaves a frame alone, SDR output
//!   is bit-reproducible at a whole-stop headroom.
//! - **Below diffuse white every peak agrees exactly**, because the lift is zero there
//!   and `r` is shared. A gain map needs the two renditions to agree below diffuse
//!   white, and here that is a property of the formula rather than a tolerance.
//! - **Mid-grey is preserved** (`r(0.18) = 0.18` at every `W`), and `headroom_stops = 0`
//!   makes reinhard the identity on every branch. The stage is the identity only where
//!   the dark end acts on nothing too.
//! - **Not bounded by the peak above `W`.** `r` keeps rising past `W` (as `v/W²`), so
//!   content brighter than the headroom exceeds the display's peak on every branch —
//!   SDR above `1.0`, HDR above `P` — and the encoder clamps and counts it. The legacy
//!   HDR renderer instead dropped `r`'s tail to stay strictly under its peak, and paid
//!   for it with a below-white disagreement between the branches; this stage takes the
//!   exact agreement. No destination adds a hard ceiling: at the default headroom no
//!   real frame reaches past the HDR peak, and what a lower headroom pushes past is
//!   clamped and counted (`nf-display-stages/branch-contract` has the measurement; for
//!   a gain map the destination clamps and counts, since `gain_ratio` does not).
//!
//! # The dark end
//!
//! **Reinhard compresses upward only**: below mid-grey it is nearly a gain (log-log
//! slope 1.00 at 0.002, 0.94 near 0.05), so without help the dark end lands wherever
//! the decode and the look's slope put it. The stage can move it, on reinhard's output
//! `y` and before the peak's lift, by one curve ([`DARK_END_CURVE`]):
//!
//! ```text
//! y′ = y · 2^(shift · (1 − smoothstep(u)))    u = (log2 y − log2 b) / (log2 0.18 − log2 b)
//! shift = log2(T / b)                          T = 0.18 · 2^−stops
//! ```
//!
//! A shift in stops, whole at the reference `b` and below (slope 1 there), fading to
//! nothing at mid-grey, so mid-grey, white and everything the peak's lift touches are
//! untouched, bit for bit, and every peak still agrees below diffuse white. Luminance
//! only, like the rest of the stage: a per-channel subtraction (the legacy
//! `--black-point`) amplifies whatever tint the shadows carry. `fit_range.dark_end` picks
//! the reference and the direction ([`DarkEnd`]):
//!
//! - **The floor** (`--dark-floor`, `nf-display-stages/midtone-contrast-toe`): `b` is the
//!   frame's measured dark end — its darkest 1 % (`roll_white::FrameStops::dark_stops`)
//!   graded by the frame's own slope and exposure — **lifted** to `T` when it renders
//!   below it, never pushed down. It holds the shadows of a steep slope (`--contrast
//!   1.3`) and of a frame wider than the placement can fit. Without measured tones it
//!   cannot act. The lift is capped at [`MAX_LIFT_SHARE`] of the stops between `b` and
//!   mid-grey, which keeps the curve's slope at or above one half (`1 − 1.5 · shift /
//!   span` at its flattest).
//! - **The target** (the default, `nf-display-stages/frame-dark-target`): the floor's
//!   reference, moved to `T` **either way** — lifted as the floor lifts, or pushed down
//!   when it renders above `T`. A push steepens the shadows; it is capped at [`MAX_PUSH_SHARE`] of the stops
//!   between `b` and mid-grey, which keeps the curve's slope at or below 2, so a
//!   high-key frame whose dark end sits near mid-grey keeps its light shadows.
//! - **Display black**: `b` is the film base, where the frame's grade renders it
//!   (`chain::render`), **pushed down** to `T` when it renders above it, never lifted
//!   (`nf-display-stages/parametric-operator`). `direct`'s pinned dark end. A base close
//!   under mid-grey squeezes the shift into a narrow band ([`ResolvedDarkEnd::warning`]).
//!
//! The three are one setting because they move the same stops. There is no
//! scene-side flare/fog subtraction (`nf-scene-correction/flare-removal`, closed as not
//! needed).
//!
//! **Reinhard is kept on purpose** (`nf-display-stages/parametric-shoulder`,
//! 2026-09-27). With mid-grey, the rendered white and the peak all pinned, any smooth
//! shoulder stays within about 0.15 stop of it. A brighter white with mid-grey pinned
//! lost to it in review. Where white renders is the look's contrast (the roll's white
//! rule). The headroom barely moves white (0.09 stop from 6 stops to 2); it sets how
//! hard the stops above white are compressed.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::algo::fixed::DIFFUSE_WHITE;
use crate::pipeline::colorimetry::dot;
use crate::pipeline::colorimetry::pinned::ACESCG_LUMA;
use crate::pipeline::look::GradedImage;
use crate::pipeline::pixels;
use crate::pipeline::working_image::WorkingBuffer;
use crate::types::{NcError, Result, headroom_fault, headroom_white_point};

/// Pinned identifier of the operator, for the report. A change to its pixels moves
/// the version.
pub const OPERATOR: &str = "reinhard-peak-lifted-v1";

/// What the report names in place of an operator when the headroom leaves nothing to
/// compress (`headroom_stops = 0`), so a report never names an operator that moved no
/// pixel.
pub const IDENTITY: &str = "identity";

/// Scene mid-grey, which the operator leaves where the decode put it.
const MID_GREY: f64 = 0.18;

/// Pinned identifier of the dark end's curve, for the report. A change to its pixels
/// moves the version.
pub const DARK_END_CURVE: &str = "log-shift-to-mid-grey-v1";

/// What the stage list names when reinhard and the dark end both run, joined in the
/// order they run, as the look joins its controls.
pub const OPERATOR_AND_DARK_END: &str = "reinhard-peak-lifted-v1+log-shift-to-mid-grey-v1";

/// The floor review chose: a frame's darkest 1 % no deeper than 4.5 stops below
/// mid-grey (L\* ≈ 7; `nf-display-stages/midtone-contrast-toe`). `--dark-floor`'s example.
pub const REVIEWED_DARK_FLOOR_STOPS: f32 = 4.5;

/// The target review chose: a frame's darkest 1 % at 5.5 stops below mid-grey
/// (L\* ≈ 3.6; `nf-display-stages/frame-dark-target`).
pub const DEFAULT_DARK_TARGET_STOPS: f32 = 5.5;

/// The depth review chose while display black was on by default: the film base 6 stops
/// below mid-grey (L\* ≈ 2.5; `nf-display-stages/parametric-operator`). `direct`'s.
pub const REVIEWED_DISPLAY_BLACK_STOPS: f32 = 6.0;

/// The deepest target accepted, in stops below mid-grey: 16 stops puts it at
/// `0.18 · 2^−16`, under a 16-bit encode's first code value.
pub const MAX_DARK_END_STOPS: f32 = 16.0;

/// The most the target pushes down, as a share of the stops between its reference and
/// mid-grey: two thirds keeps the curve's log-log slope at or below 2, the mirror of
/// [`MAX_LIFT_SHARE`].
pub const MAX_PUSH_SHARE: f64 = 2.0 / 3.0;

/// The most the floor lifts, as a share of the stops between its reference and
/// mid-grey: a third keeps the curve's log-log slope at or above one half.
pub const MAX_LIFT_SHARE: f64 = 1.0 / 3.0;

/// Where the dark end renders on the display — the recipe's `fit_range.dark_end`, set by
/// `--dark-floor`, `--dark-target` or `--display-black`: `"off"`, `{"floor": stops}`,
/// `{"target": stops}` or `{"black": stops}`, stops below mid-grey.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DarkEnd {
    /// Left where the decode and the look put it.
    Off,
    /// The frame's measured dark end lifted to no deeper than `0.18 · 2^−stops`.
    Floor(f32),
    /// The frame's measured dark end placed at `0.18 · 2^−stops`, lifted or pushed down.
    Target(f32),
    /// The film base pushed down to `0.18 · 2^−stops`.
    Black(f32),
}

/// The target at its reviewed depth.
impl Default for DarkEnd {
    fn default() -> Self {
        DarkEnd::Target(DEFAULT_DARK_TARGET_STOPS)
    }
}

/// A [`DarkEnd`] whose stops are not finite, or not in `(0, MAX_DARK_END_STOPS]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DarkEndFault(pub f32);

/// The literal `"off"`, spelled once for the flags.
const OFF: &str = "off";

impl DarkEnd {
    /// The value rule, rendered by the recipe gate as a usage error.
    pub fn check(self) -> std::result::Result<(), DarkEndFault> {
        match self {
            DarkEnd::Floor(v) | DarkEnd::Target(v) | DarkEnd::Black(v)
                if !(v > 0.0 && v <= MAX_DARK_END_STOPS) =>
            {
                Err(DarkEndFault(v))
            }
            _ => Ok(()),
        }
    }

    /// `--dark-floor`'s spelling: stops below mid-grey, or `off`. The range is the
    /// recipe gate's, so the flag and the key share one message.
    pub fn parse_floor(s: &str) -> std::result::Result<Self, String> {
        Self::parse(s, DarkEnd::Floor, REVIEWED_DARK_FLOOR_STOPS)
    }

    /// `--dark-target`'s spelling, as [`DarkEnd::parse_floor`].
    pub fn parse_target(s: &str) -> std::result::Result<Self, String> {
        Self::parse(s, DarkEnd::Target, DEFAULT_DARK_TARGET_STOPS)
    }

    /// The flag that sets this, and its name in prose; `None` for `off`.
    pub fn names(self) -> Option<(&'static str, &'static str)> {
        match self {
            DarkEnd::Off => None,
            DarkEnd::Floor(_) => Some(("--dark-floor", "dark floor")),
            DarkEnd::Target(_) => Some(("--dark-target", "dark target")),
            DarkEnd::Black(_) => Some(("--display-black", "display black")),
        }
    }

    /// Whether this setting reads the frame's measured dark end (the floor, the target).
    pub fn reads_frame_dark(self) -> bool {
        matches!(self, DarkEnd::Floor(_) | DarkEnd::Target(_))
    }

    /// `--display-black`'s spelling, as [`DarkEnd::parse_floor`].
    pub fn parse_black(s: &str) -> std::result::Result<Self, String> {
        Self::parse(s, DarkEnd::Black, REVIEWED_DISPLAY_BLACK_STOPS)
    }

    fn parse(s: &str, on: fn(f32) -> Self, example: f32) -> std::result::Result<Self, String> {
        if s == OFF {
            return Ok(DarkEnd::Off);
        }
        s.parse::<f32>().map(on).map_err(|_| {
            format!("expected stops below mid-grey (e.g. {example}) or `{OFF}`, got {s:?}")
        })
    }

    /// The target's luminance on the display, when on.
    fn target(self) -> Option<f64> {
        match self {
            DarkEnd::Off => None,
            DarkEnd::Floor(stops) | DarkEnd::Target(stops) | DarkEnd::Black(stops) => {
                Some(MID_GREY * (-f64::from(stops)).exp2())
            }
        }
    }
}

/// The display's peak, relative to reference white (display `1.0`) — the one argument
/// the two display branches differ in.
///
/// Checked, so an unusable peak cannot reach the operator: below `1.0` the lift would
/// *darken* above diffuse white, and a non-finite one poisons every bright pixel.
/// Bounded above by PQ's 10,000 nits over the 203-nit reference white, the brightest
/// signal any display destination can carry.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DisplayPeak(f32);

impl DisplayPeak {
    /// An SDR display: reference white is the peak.
    pub const SDR: Self = DisplayPeak(1.0);

    /// The largest representable peak: PQ's 10,000 nits over 203-nit reference white.
    const MAX: f32 = 10_000.0 / 203.0;

    /// Check a peak, or refuse it.
    pub fn new(peak: f32) -> Result<Self> {
        if !peak.is_finite() || !(1.0..=Self::MAX).contains(&peak) {
            return Err(NcError::Other(format!(
                "fit range's display peak must be finite and within [1, {}] times \
                 reference white, got {peak}",
                Self::MAX
            )));
        }
        Ok(DisplayPeak(peak))
    }

    /// The peak as a multiple of reference white.
    pub fn value(self) -> f32 {
        self.0
    }
}

/// Fit range's parameters: the recipe's `fit_range` section plus the destination's
/// peak. **No `Default`**, for the reason [`FitGamutParams`] has none: the peak is the
/// destination's to state.
///
/// [`FitGamutParams`]: crate::pipeline::fit_gamut::FitGamutParams
#[derive(Clone, Debug, PartialEq)]
pub struct FitRangeParams {
    /// How much scene range above diffuse white the operator compresses, in stops:
    /// reinhard's white point is `W = 2^headroom_stops`. `0` is the identity.
    pub headroom_stops: f32,
    /// The display's peak.
    pub peak: DisplayPeak,
    /// Where the dark end renders on the display.
    pub dark_end: DarkEnd,
    /// The film base's **graded** ACEScg luminance — the decoded base run through the
    /// frame's own scene correction and look, which `chain::render` does. Display
    /// black's reference, and otherwise `pipeline::black_endpoint`'s.
    pub film_base: f32,
    /// The frame's measured dark end, graded the same way (`chain::graded_tone`): the
    /// floor's reference. `None` without measured tones.
    pub frame_dark: Option<f32>,
}

impl FitRangeParams {
    /// Reinhard's white point, `2^headroom_stops`.
    fn white_point(&self) -> f32 {
        headroom_white_point(self.headroom_stops)
    }

    /// What fit range resolved, for the report — the operator by name and its
    /// arguments as values, so a report never describes the fit in prose.
    pub fn resolved(&self) -> FitRange {
        let white_point = self.white_point();
        FitRange {
            // Reinhard's own identity test, so the report never names an operator that
            // moved no pixel. `apply` skips the pixels only when the dark end moves
            // none either; [`FitRange::applied`] joins the two for the stage list.
            operator: if white_point == 1.0 {
                IDENTITY
            } else {
                OPERATOR
            },
            headroom_stops: self.headroom_stops,
            white_point,
            display_peak: self.peak,
            dark_end: self.dark_end().0,
        }
    }

    /// The dark end, resolved against where its reference renders without it: the
    /// report's account, and the shift `apply` runs (`None` when off or left alone).
    fn dark_end(&self) -> (ResolvedDarkEnd, Option<Shift>) {
        let white_point = self.white_point();
        let gain = mid_grey_preserving_gain(white_point);
        let render = |y: f32| f64::from(reinhard(y, white_point, gain));
        let frame_dark = self
            .frame_dark
            .filter(|y| y.is_finite() && *y > 0.0)
            .map(render);
        // The reference and the shift in stops at it: black pushes the base down, the
        // floor lifts the frame's dark end, the target moves it either way.
        let shift = match self.dark_end {
            DarkEnd::Off => None,
            DarkEnd::Black(_) => {
                let base = render(self.film_base);
                self.dark_end
                    .target()
                    .filter(|&target| base > target && base < MID_GREY)
                    .map(|target| (base, (target / base).log2()))
            }
            DarkEnd::Floor(_) => frame_dark
                .zip(self.dark_end.target())
                .filter(|&(dark, target)| dark < target)
                .map(|(dark, target)| {
                    let span = (MID_GREY / dark).log2();
                    (dark, (target / dark).log2().min(span * MAX_LIFT_SHARE))
                }),
            DarkEnd::Target(_) => frame_dark
                .zip(self.dark_end.target())
                .filter(|&(dark, target)| dark != target && dark < MID_GREY)
                .map(|(dark, target)| {
                    let span = (MID_GREY / dark).log2();
                    let stops = (target / dark).log2();
                    (
                        dark,
                        stops.clamp(-span * MAX_PUSH_SHARE, span * MAX_LIFT_SHARE),
                    )
                }),
        }
        .map(|(reference, stops)| Shift {
            stops,
            whole: stops.exp2(),
            base: reference.log2(),
            knee: MID_GREY.log2(),
        });
        let resolved = ResolvedDarkEnd {
            setting: self.dark_end,
            curve: if shift.is_some() {
                DARK_END_CURVE
            } else {
                IDENTITY
            },
            film_base_stops: -display_stops(self.headroom_stops, self.film_base),
            frame_dark_stops: frame_dark.map(|y| (MID_GREY / y).log2() as f32),
            shift_stops: shift.map_or(0.0, |s| s.stops as f32),
        };
        (resolved, shift)
    }
}

/// What the dark end did to a frame, for the report.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ResolvedDarkEnd {
    /// The knob as set.
    pub setting: DarkEnd,
    /// [`DARK_END_CURVE`], or `"identity"` when it is off, its reference already renders
    /// on the target's side (left alone), the frame has no measured dark end, or display
    /// black's base renders at or above mid-grey, where a shift that holds mid-grey
    /// cannot reach it ([`ResolvedDarkEnd::warning`]).
    pub curve: &'static str,
    /// Where the film base renders without the dark end placed, in stops below mid-grey
    /// on the display.
    pub film_base_stops: f32,
    /// Where the frame's measured dark end renders without it, likewise; `null`
    /// without measured tones.
    pub frame_dark_stops: Option<f32>,
    /// The shift applied at the reference, in stops (negative darkens); `0` when left
    /// alone.
    pub shift_stops: f32,
}

/// How far under mid-grey the film base must render for display black to place it
/// well. The shift is spread between the base and mid-grey, so a base closer than this
/// packs the whole shift into a narrow band — at 2 stops under and a 6-stop target,
/// a 4-stop shift over 2 stops steepens the band's middle 4× — and at mid-grey or
/// above black cannot run at all. Only a strong exposure gets a base here (the numbers
/// for the default contrast: `docs/using-nc.md`, "The dark end").
pub const MIN_FILM_BASE_STOPS: f32 = 2.0;

impl ResolvedDarkEnd {
    /// A warning when display black is on but the film base renders less than
    /// [`MIN_FILM_BASE_STOPS`] under mid-grey — crushed into a narrow band below it,
    /// or (at or above mid-grey) not placed at all — or when the lift's cap
    /// ([`MAX_LIFT_SHARE`]) leaves the frame's dark end under it. `None` otherwise.
    pub fn warning(&self) -> Option<String> {
        if let (DarkEnd::Floor(floor) | DarkEnd::Target(floor), Some(dark)) =
            (self.setting, self.frame_dark_stops)
            && let Some((_, what)) = self.setting.names()
        {
            let lands = dark - self.shift_stops;
            // A rounding's worth past the floor is the floor reached. Only a capped lift
            // lands deeper; a capped push leaves a high-key frame's shadows light.
            return (lands > floor + 1e-3).then(|| {
                format!(
                    "the frame's darkest 1 % renders {dark:.2} stops under mid-grey, so far \
                     that the {what}'s raise stops at its cap (a third of the way to \
                     mid-grey, so the shadows keep at least half their contrast): it lands \
                     {lands:.2} stops under, below the {what}'s {floor}. The frame is very \
                     steep or very dark: lower `--contrast` (recipe `look.contrast`) or raise \
                     `--exposure` (recipe `scene_correction.exposure`)"
                )
            });
        }
        if !matches!(self.setting, DarkEnd::Black(_)) || self.film_base_stops >= MIN_FILM_BASE_STOPS
        {
            return None;
        }
        let (depth, what) = if self.curve == IDENTITY {
            (
                format!("{:.2} stops above", -self.film_base_stops),
                "a shift that holds mid-grey cannot reach it, so display black was skipped",
            )
        } else {
            (
                format!("only {:.2} stops under", self.film_base_stops),
                "so display black squeezed its whole shift into that narrow band, crushing \
                 the shadows below mid-grey",
            )
        };
        Some(format!(
            "the film base renders {depth} mid-grey, closer than the {MIN_FILM_BASE_STOPS} \
             stops under it display black needs: {what}. The frame is placed very bright: \
             lower `--exposure` (recipe `scene_correction.exposure`), or pass \
             `--display-black off` (recipe `fit_range.dark_end` \"off\") to leave black \
             where the grade puts it"
        ))
    }

    /// Whether the floor or the target is set but the frame has no measured dark end to
    /// hold.
    pub fn dark_unread(&self) -> bool {
        self.setting.reads_frame_dark() && self.frame_dark_stops.is_none()
    }
}

/// Where reinhard puts a graded luminance with the dark end left alone, in stops from
/// mid-grey (negative below): every peak agrees below diffuse white, so no peak is taken.
pub fn display_stops(headroom_stops: f32, luminance: f32) -> f32 {
    let white_point = headroom_white_point(headroom_stops);
    let y = reinhard(
        luminance,
        white_point,
        mid_grey_preserving_gain(white_point),
    );
    -(MID_GREY / f64::from(y)).log2() as f32
}

/// What fit range applied to a frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct FitRange {
    /// [`OPERATOR`], or `"identity"` when the headroom leaves nothing to compress.
    pub operator: &'static str,
    pub headroom_stops: f32,
    /// Reinhard's white point, `2^headroom_stops`.
    pub white_point: f32,
    /// The display's peak, as a multiple of reference white.
    pub display_peak: DisplayPeak,
    /// Where the dark end landed on the display.
    pub dark_end: ResolvedDarkEnd,
}

impl FitRange {
    /// The stage's entry in the report's list, and the tone curve an HDR destination
    /// names: what ran, joined by `+` in the order it ran — reinhard, then the dark
    /// end — or `"identity"` when neither moves a pixel (the tests `apply` skips on).
    pub fn applied(&self) -> &'static str {
        match (self.operator != IDENTITY, self.dark_end.curve != IDENTITY) {
            (true, true) => OPERATOR_AND_DARK_END,
            (true, false) => self.operator,
            (false, true) => self.dark_end.curve,
            (false, false) => IDENTITY,
        }
    }
}

/// Pixels whose **luminance** now fits the display's range, before the gamut is
/// fitted — and the peak they were fitted to.
///
/// A separate boundary from [`DisplayReferredImage`] because the two stages are
/// coupled but distinct: the gamut ceiling follows the range this stage produced,
/// which is why they stay adjacent rather than merged. The peak rides here so fit
/// gamut reads the one this stage used rather than being handed it a second time.
///
/// [`DisplayReferredImage`]: crate::pipeline::fit_gamut::DisplayReferredImage
pub struct RangeFittedImage(WorkingBuffer, DisplayPeak);

impl RangeFittedImage {
    /// The display peak these pixels were fitted to.
    pub fn peak(&self) -> DisplayPeak {
        self.1
    }

    /// Hand the buffer to the next stage. Consuming, so the pixels move rather
    /// than copy.
    pub(in crate::pipeline) fn into_buffer(self) -> WorkingBuffer {
        self.0
    }
}

impl fmt::Debug for RangeFittedImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_named(f, "RangeFittedImage")
    }
}

/// Fit the scene's range to the display's, in place.
///
/// **Refuses a non-finite sample**, naming the lowest such pixel: a NaN or infinity
/// here is an upstream fault, and scaling by it would hand the encoder a value it can
/// only clamp. Checked at every setting, the identity included, so whether a frame
/// renders never depends on the headroom. The headroom is checked by `types`'
/// [`headroom_fault`], the rule the recipe's validation reports; reaching here with a
/// bad one is a wiring bug.
pub fn apply(image: GradedImage, params: &FitRangeParams) -> Result<RangeFittedImage> {
    if let Some(fault) = headroom_fault(params.headroom_stops) {
        return Err(NcError::Other(format!(
            "fit range was handed an unusable headroom ({fault:?}); the recipe's \
             validation should have refused it"
        )));
    }
    // Only read when display black is on, so only refused then: turning black off must
    // not leave a failure over a value the render never uses.
    if matches!(params.dark_end, DarkEnd::Black(_))
        && !(params.film_base.is_finite() && params.film_base > 0.0)
    {
        return Err(NcError::Other(format!(
            "fit range was handed a film base that graded to luminance {}; the recipe's \
             validation should have refused it (`recipe::validate_render`)",
            params.film_base
        )));
    }
    let mut buffer = image.into_buffer();
    let white_point = params.white_point();
    let dark_end = params.dark_end().1;
    let identity = white_point == 1.0 && dark_end.is_none();
    let operator = Operator::new(white_point, params.headroom_stops, params.peak, dark_end);
    pixels::try_map_in_place(buffer.rgb_mut(), |index, px| {
        if !px.iter().all(|v| v.is_finite()) {
            return Err(NcError::Other(format!(
                "fit range received a non-finite sample at pixel {index} ({px:?})"
            )));
        }
        if identity {
            return Ok(());
        }
        let luminance = dot(*px, ACESCG_LUMA);
        let scale = operator.scale(luminance);
        for channel in px.iter_mut() {
            *channel *= scale;
        }
        if !px.iter().all(|v| v.is_finite()) {
            return Err(NcError::Other(format!(
                "fit range produced a non-finite sample at pixel {index} from luminance \
                 {luminance}"
            )));
        }
        Ok(())
    })?;
    Ok(RangeFittedImage(buffer, params.peak))
}

/// The operator with its per-frame constants resolved once.
#[derive(Clone, Copy, Debug)]
struct Operator {
    white_point: f32,
    /// The input gain that keeps mid-grey at mid-grey.
    gain: f64,
    /// `log2` of diffuse white, where the lift starts.
    bottom_stops: f32,
    /// `log2(W)`, i.e. the headroom in stops — taken from the knob rather than
    /// recomputed, so the lift's end costs no libm call.
    top_stops: f32,
    peak: f32,
    dark_end: Option<Shift>,
}

impl Operator {
    fn new(
        white_point: f32,
        headroom_stops: f32,
        peak: DisplayPeak,
        dark_end: Option<Shift>,
    ) -> Self {
        Self {
            white_point,
            gain: mid_grey_preserving_gain(white_point),
            bottom_stops: DIFFUSE_WHITE.log2(),
            top_stops: headroom_stops,
            peak: peak.value(),
            dark_end,
        }
    }

    /// The factor a pixel of this luminance is scaled by: `f(Y)/Y`.
    ///
    /// **At `Y ≤ 0` it is the curve's limit at black, the mid-grey gain** — not `1` —
    /// times the dark end's whole shift. `f` is a pure gain near zero (`f(Y)/Y → gain`,
    /// ≈1.22 at six stops, and the shift is a constant factor below the base), so
    /// leaving those pixels unscaled would step every channel where a saturated
    /// colour's luminance crosses zero. The limit keeps the scale continuous, and still
    /// clamps nothing.
    fn scale(self, luminance: f32) -> f32 {
        if luminance <= 0.0 {
            let whole = self.dark_end.map_or(1.0, |s| s.whole);
            return (self.gain * whole) as f32;
        }
        self.apply(luminance) / luminance
    }

    /// `r(v) · (1 + (P − 1)·s(v))` for a positive luminance.
    fn apply(self, value: f32) -> f32 {
        let base = reinhard(value, self.white_point, self.gain);
        let base = match self.dark_end {
            Some(shift) => shift.apply(base),
            None => base,
        };
        // Skipped rather than multiplied by one: bit-identical, and it keeps the `log2`
        // off the SDR branch and off everything below diffuse white.
        if self.peak == 1.0 || value <= DIFFUSE_WHITE {
            return base;
        }
        base * (1.0 + (self.peak - 1.0) * self.lift(value))
    }

    /// The lift's ramp: `0` at diffuse white, `1` at the white point, smooth at both
    /// ends so the lifted curve joins `r` without a crease. In stops, because a ramp in
    /// linear value would spend nearly all of its span in the top stop.
    fn lift(self, value: f32) -> f32 {
        let span = self.top_stops - self.bottom_stops;
        // No span above diffuse white: nothing to lift across (zero headroom).
        if span <= 0.0 {
            return 0.0;
        }
        let t = ((value.log2() - self.bottom_stops) / span).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

/// The dark end's shift, resolved once per frame: `stops` at and below `base` (the
/// reference), fading with a smoothstep in `log2` to nothing at `knee`, mid-grey. `base`
/// and `knee` are `log2` of display luminance.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shift {
    stops: f64,
    /// `2^stops`, the factor at and below the base — resolved once, for `Y ≤ 0`.
    whole: f64,
    base: f64,
    knee: f64,
}

impl Shift {
    /// Shift one of reinhard's outputs. Only values under the knee pay the `log2` and
    /// `exp2`; from mid-grey up the value comes back untouched, bit for bit.
    fn apply(self, y: f32) -> f32 {
        let v = f64::from(y);
        if v >= MID_GREY {
            return y;
        }
        debug_assert!(v > 0.0, "`Operator::scale` handles Y ≤ 0 before the shift");
        let u = ((v.log2() - self.base) / (self.knee - self.base)).clamp(0.0, 1.0);
        (v * (self.stops * (1.0 - u * u * (3.0 - 2.0 * u))).exp2()) as f32
    }
}

/// Extended reinhard with an input gain: `u·(1 + u/W²)/(1 + u)` over `u = gain·v`.
///
/// Binary64 so a large `v` keeps its `u/W²` term; multiply and divide are IEEE-exact,
/// so this is bit-reproducible across targets. Monotonic for every `W > 0`.
fn reinhard(value: f32, white_point: f32, gain: f64) -> f32 {
    let u = f64::from(value) * gain;
    let w = f64::from(white_point);
    (u * (1.0 + u / (w * w)) / (1.0 + u)) as f32
}

/// The input gain that makes [`reinhard`] return [`MID_GREY`] at mid-grey.
///
/// Solving `r(x) = m` gives `x² + (1−m)W²x − mW² = 0`; the gain is `x/m`, written in the
/// rationalized form `2 / ((1−m) + √((1−m)² + 4m/W²))`, which has no subtraction to
/// cancel at large `W`. Exactly `1` at `W = 1`, where the curve is the identity. `sqrt`
/// is correctly rounded by IEEE 754, so this stays target-independent.
fn mid_grey_preserving_gain(white_point: f32) -> f64 {
    let inv_w2 = 1.0 / (f64::from(white_point) * f64::from(white_point));
    let k = 1.0 - MID_GREY;
    2.0 / (k + (k * k + 4.0 * MID_GREY * inv_w2).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DEFAULT_HEADROOM_STOPS, MAX_HEADROOM_STOPS};

    /// An HDR display's peak for these tests: 1000 nits over 203-nit reference white.
    const HDR_PEAK: f32 = 1000.0 / 203.0;

    fn op(stops: f32, peak: f32) -> Operator {
        Operator::new(
            headroom_white_point(stops),
            stops,
            DisplayPeak::new(peak).unwrap(),
            None,
        )
    }

    /// A log-spaced sweep from deep shadow to far past any white point.
    fn sweep() -> impl Iterator<Item = f32> {
        (0..=400).map(|i| 2f32.powf(-12.0 + i as f32 * 0.05))
    }

    #[test]
    fn mid_grey_is_preserved_at_every_headroom_and_peak() {
        for stops in [0.0, 1.0, 4.0, 6.0, 8.0, 12.0, MAX_HEADROOM_STOPS] {
            for peak in [1.0, HDR_PEAK] {
                let out = op(stops, peak).apply(0.18);
                assert!(
                    (out - 0.18).abs() < 1e-6,
                    "stops {stops}, peak {peak}: {out}"
                );
            }
        }
    }

    #[test]
    fn zero_headroom_is_the_identity_on_every_branch() {
        for peak in [1.0, HDR_PEAK] {
            let o = op(0.0, peak);
            for v in sweep() {
                assert_eq!(o.apply(v).to_bits(), v.to_bits(), "peak {peak}, v {v}");
            }
        }
    }

    #[test]
    fn every_peak_agrees_exactly_below_diffuse_white() {
        // The gain-map contract, as a property of the formula: bit-identical, not
        // merely within a code step.
        let sdr = op(DEFAULT_HEADROOM_STOPS, 1.0);
        for peak in [1.5, HDR_PEAK, DisplayPeak::MAX] {
            let hdr = op(DEFAULT_HEADROOM_STOPS, peak);
            for v in sweep().filter(|&v| v <= DIFFUSE_WHITE) {
                assert_eq!(hdr.apply(v).to_bits(), sdr.apply(v).to_bits(), "{v}");
            }
            // And the lift is real above it — the control that makes the equality
            // above mean something.
            assert!(hdr.apply(4.0) > sdr.apply(4.0) * 1.1, "peak {peak}");
        }
    }

    #[test]
    fn it_is_monotonic_and_finite() {
        for stops in [0.5, 2.0, DEFAULT_HEADROOM_STOPS, 12.0, MAX_HEADROOM_STOPS] {
            for peak in [1.0, 1.01, HDR_PEAK, DisplayPeak::MAX] {
                let o = op(stops, peak);
                let mut last = 0.0;
                for v in sweep().chain([1e6, 1e20]) {
                    let out = o.apply(v);
                    assert!(out.is_finite(), "stops {stops}, peak {peak}, v {v}");
                    assert!(out >= last, "stops {stops}, peak {peak}: falls at {v}");
                    last = out;
                }
            }
        }
    }

    #[test]
    fn the_white_point_lands_at_the_peak_and_content_above_it_overshoots() {
        // At `W` the lift is complete, so the output is the peak times `r(W)` — just
        // over it, as the SDR curve is just over `1.0` there. Past `W` the tail keeps
        // rising on every branch: the documented, encoder-counted overshoot.
        let w = headroom_white_point(DEFAULT_HEADROOM_STOPS);
        for peak in [1.0, HDR_PEAK] {
            let o = op(DEFAULT_HEADROOM_STOPS, peak);
            let at_w = o.apply(w);
            assert!(at_w > peak && at_w < peak * 1.01, "peak {peak}: {at_w}");
            assert!(o.apply(8.0 * w) > at_w, "peak {peak}");
        }
    }

    #[test]
    fn the_lift_joins_the_base_without_a_crease() {
        // The lifted curve's slope just above diffuse white matches the shared one
        // just below it — smoothstep has zero slope at `t = 0`.
        let o = op(DEFAULT_HEADROOM_STOPS, HDR_PEAK);
        let h = 1e-3;
        let below = (o.apply(DIFFUSE_WHITE) - o.apply(DIFFUSE_WHITE - h)) / h;
        let above = (o.apply(DIFFUSE_WHITE + h) - o.apply(DIFFUSE_WHITE)) / h;
        assert!((above - below).abs() / below < 0.02, "{below} vs {above}");
    }

    #[test]
    fn reinhard_compresses_upward_only() {
        // Below mid-grey the curve is nearly a gain: its log-log slope is 1 deep in the
        // shadows and still 0.94 near 0.05 — which is why display black shapes the
        // shadow end (module docs). Pinned so a change there is a decision.
        let o = op(DEFAULT_HEADROOM_STOPS, 1.0);
        let slope = |v: f32| (o.apply(v * 1.001) / o.apply(v)).ln() / 1.001f32.ln();
        assert!((slope(0.002) - 1.0).abs() < 0.01, "{}", slope(0.002));
        assert!((slope(0.05) - 0.94).abs() < 0.01, "{}", slope(0.05));
        assert!((slope(1.0) - 0.45).abs() < 0.02, "{}", slope(1.0));
    }

    #[test]
    fn the_peak_is_checked() {
        assert!(DisplayPeak::new(1.0).is_ok());
        assert!(DisplayPeak::new(HDR_PEAK).is_ok());
        for bad in [
            0.99,
            0.0,
            -1.0,
            f32::NAN,
            f32::INFINITY,
            DisplayPeak::MAX * 1.01,
        ] {
            assert!(DisplayPeak::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_scale_is_continuous_where_luminance_crosses_zero() {
        // Just above zero the curve is a pure gain, so its scale meets the one a pixel
        // at or below zero gets — no step for a saturated colour straddling black.
        for peak in [1.0, HDR_PEAK] {
            let o = op(DEFAULT_HEADROOM_STOPS, peak);
            let below = o.scale(-1e-3);
            assert_eq!(below, o.scale(0.0));
            for above in [1e-9, 1e-6] {
                assert!(
                    (o.scale(above) - below).abs() / below < 1e-4,
                    "peak {peak}: {} vs {below}",
                    o.scale(above)
                );
            }
            assert!(
                below > 1.2,
                "the limit is the mid-grey gain, not 1: {below}"
            );
        }
        // And at zero headroom the gain is exactly 1.
        assert_eq!(op(0.0, 1.0).scale(-1.0), 1.0);
    }

    #[test]
    fn the_headroom_rule_is_the_shared_one() {
        use crate::types::{HeadroomFault, MAX_HEADROOM_STOPS};
        assert_eq!(headroom_fault(0.0), None);
        assert_eq!(headroom_fault(MAX_HEADROOM_STOPS), None);
        assert_eq!(headroom_fault(-0.5), Some(HeadroomFault::Negative(-0.5)));
        assert!(matches!(
            headroom_fault(f32::NAN),
            Some(HeadroomFault::Negative(_))
        ));
        assert_eq!(headroom_fault(25.0), Some(HeadroomFault::TooLarge(25.0)));
    }

    /// Fit range at the default headroom with the dark end set, for a film base whose
    /// graded luminance is `film_base` and no measured dark end.
    fn with_black(dark_end: DarkEnd, film_base: f32, peak: f32) -> FitRangeParams {
        FitRangeParams {
            headroom_stops: DEFAULT_HEADROOM_STOPS,
            peak: DisplayPeak::new(peak).unwrap(),
            dark_end,
            film_base,
            frame_dark: None,
        }
    }

    /// The floor at `stops`, for a frame whose graded dark end is `frame_dark`.
    fn with_floor(stops: f32, frame_dark: Option<f32>, peak: f32) -> FitRangeParams {
        FitRangeParams {
            frame_dark,
            ..with_black(DarkEnd::Floor(stops), CAPPED_BASE, peak)
        }
    }

    fn operator(p: &FitRangeParams) -> Operator {
        Operator::new(p.white_point(), p.headroom_stops, p.peak, p.dark_end().1)
    }

    const BLACK: DarkEnd = DarkEnd::Black(REVIEWED_DISPLAY_BLACK_STOPS);

    /// A capped roll's film base: it renders ≈ 4.4 stops under mid-grey without black.
    const CAPPED_BASE: f32 = 0.0075;

    /// A steep frame's darkest 1 %: it renders ≈ 5.2 stops under mid-grey, under the
    /// 4.5-stop floor.
    const STEEP_DARK: f32 = 0.004;

    #[test]
    fn display_black_lands_the_film_base_on_its_target() {
        for stops in [4.5_f32, 6.0, 7.5] {
            let p = with_black(DarkEnd::Black(stops), CAPPED_BASE, 1.0);
            let got = f64::from(operator(&p).apply(CAPPED_BASE));
            let want = MID_GREY * (-f64::from(stops)).exp2();
            assert!((got / want - 1.0).abs() < 1e-5, "{stops}: {got} vs {want}");
            let r = p.resolved().dark_end;
            assert_eq!(r.curve, DARK_END_CURVE);
            // The shift darkens, so the base ends up `film_base_stops − shift_stops` below mid.
            assert!(
                (r.film_base_stops - r.shift_stops - stops).abs() < 1e-4,
                "{r:?}"
            );
        }
    }

    #[test]
    fn the_floor_lifts_the_frame_s_dark_end_to_its_target() {
        for stops in [4.0_f32, REVIEWED_DARK_FLOOR_STOPS, 5.0] {
            let p = with_floor(stops, Some(STEEP_DARK), 1.0);
            let got = f64::from(operator(&p).apply(STEEP_DARK));
            let want = MID_GREY * (-f64::from(stops)).exp2();
            assert!((got / want - 1.0).abs() < 1e-5, "{stops}: {got} vs {want}");
            let r = p.resolved().dark_end;
            assert_eq!(r.curve, DARK_END_CURVE);
            let dark = r.frame_dark_stops.expect("measured");
            assert!(r.shift_stops > 0.0, "a lift: {r:?}");
            assert!((dark - r.shift_stops - stops).abs() < 1e-4, "{r:?}");
            // The film base is reported, not moved toward anything.
            assert!((r.film_base_stops - 4.4).abs() < 0.1, "{r:?}");
        }
    }

    #[test]
    fn the_floor_leaves_a_dark_end_on_or_above_it_alone() {
        let plain = op(DEFAULT_HEADROOM_STOPS, 1.0);
        // 0.0075 renders ≈ 4.4 stops under mid-grey: above a 4.5-stop floor.
        for p in [
            with_floor(REVIEWED_DARK_FLOOR_STOPS, Some(CAPPED_BASE), 1.0),
            with_floor(REVIEWED_DARK_FLOOR_STOPS, None, 1.0),
        ] {
            let r = p.resolved().dark_end;
            assert_eq!(r.curve, IDENTITY, "{r:?}");
            assert_eq!(r.shift_stops, 0.0, "{r:?}");
            for v in sweep() {
                assert_eq!(operator(&p).apply(v).to_bits(), plain.apply(v).to_bits());
            }
        }
        // Without measured tones the floor has nothing to hold, and says so.
        let unread = with_floor(REVIEWED_DARK_FLOOR_STOPS, None, 1.0)
            .resolved()
            .dark_end;
        assert!(unread.dark_unread() && unread.frame_dark_stops.is_none());
        let read = with_floor(REVIEWED_DARK_FLOOR_STOPS, Some(CAPPED_BASE), 1.0)
            .resolved()
            .dark_end;
        assert!(!read.dark_unread());
        assert!(
            !with_black(BLACK, CAPPED_BASE, 1.0)
                .resolved()
                .dark_end
                .dark_unread()
        );
    }

    /// The target at `stops`, for a frame whose graded dark end is `frame_dark`.
    fn with_target(stops: f32, frame_dark: f32) -> FitRangeParams {
        FitRangeParams {
            frame_dark: Some(frame_dark),
            ..with_black(DarkEnd::Target(stops), CAPPED_BASE, 1.0)
        }
    }

    #[test]
    fn the_target_moves_the_frame_s_dark_end_either_way_onto_it() {
        // CAPPED_BASE renders ≈ 4.4 stops under (pushed), STEEP_DARK ≈ 5.2 (lifted at 5,
        // pushed at 6).
        for (dark, stops) in [
            (CAPPED_BASE, 5.0_f32),
            (CAPPED_BASE, 6.0),
            (STEEP_DARK, 5.0),
            (STEEP_DARK, 6.0),
        ] {
            let p = with_target(stops, dark);
            let got = f64::from(operator(&p).apply(dark));
            let want = MID_GREY * (-f64::from(stops)).exp2();
            assert!(
                (got / want - 1.0).abs() < 1e-5,
                "{dark} at {stops}: {got} vs {want}"
            );
            let r = p.resolved().dark_end;
            assert_eq!(r.curve, DARK_END_CURVE);
            assert!(r.warning().is_none(), "{r:?}");
        }
    }

    #[test]
    fn the_target_caps_its_push_at_two_thirds_of_the_way_to_mid_grey() {
        // 0.09 renders ≈ 1.2 stops under mid-grey: a 6-stop target would need ≈ 4.8.
        let p = with_target(6.0, 0.09);
        let r = p.resolved().dark_end;
        let dark = f64::from(r.frame_dark_stops.expect("measured"));
        let shift = f64::from(r.shift_stops);
        assert!((shift + dark * MAX_PUSH_SHARE).abs() < 1e-4, "{r:?}");
        // Capped short of the target, and lighter than it: no warning.
        assert!(r.warning().is_none(), "{r:?}");
        // The curve's log-log slope stays at or under 2 across the shift's band.
        let o = operator(&p);
        let plain = op(DEFAULT_HEADROOM_STOPS, 1.0);
        let samples: Vec<f64> = (0..=200)
            .map(|i| 0.01 * (18.0_f64).powf(f64::from(i) / 200.0))
            .collect();
        for w in samples.windows(2) {
            let (a, b) = (w[0] as f32, w[1] as f32);
            let out = f64::from(o.apply(b)).log2() - f64::from(o.apply(a)).log2();
            let input = f64::from(plain.apply(b)).log2() - f64::from(plain.apply(a)).log2();
            assert!(out / input <= 2.0 + 1e-3, "at {a}: {}", out / input);
        }
    }

    #[test]
    fn the_target_leaves_a_dark_end_at_or_above_mid_grey_alone() {
        let plain = op(DEFAULT_HEADROOM_STOPS, 1.0);
        let p = with_target(6.0, 0.25);
        let r = p.resolved().dark_end;
        assert_eq!((r.curve, r.shift_stops), (IDENTITY, 0.0), "{r:?}");
        for v in sweep() {
            assert_eq!(operator(&p).apply(v).to_bits(), plain.apply(v).to_bits());
        }
    }

    #[test]
    fn the_dark_end_leaves_mid_grey_and_everything_above_it_untouched() {
        let plain = op(DEFAULT_HEADROOM_STOPS, 1.0);
        for (p, darker) in [
            (with_black(BLACK, CAPPED_BASE, 1.0), true),
            (
                with_floor(REVIEWED_DARK_FLOOR_STOPS, Some(STEEP_DARK), 1.0),
                false,
            ),
            (with_target(6.0, CAPPED_BASE), true),
        ] {
            let o = operator(&p);
            // Reinhard maps mid-grey to itself, so every input from 0.18 up renders at or
            // above the knee.
            for v in sweep().filter(|v| *v >= 0.18) {
                assert_eq!(o.apply(v).to_bits(), plain.apply(v).to_bits(), "at {v}");
            }
            assert_eq!(o.apply(0.1) < plain.apply(0.1), darker, "a shadow moves");
        }
    }

    #[test]
    fn the_dark_end_keeps_slope_one_below_its_reference() {
        let plain = op(DEFAULT_HEADROOM_STOPS, 1.0);
        for (p, reference) in [
            (with_black(BLACK, CAPPED_BASE, 1.0), CAPPED_BASE),
            (
                with_floor(REVIEWED_DARK_FLOOR_STOPS, Some(STEEP_DARK), 1.0),
                STEEP_DARK,
            ),
        ] {
            let o = operator(&p);
            let ratio = |v: f32| o.apply(v) / plain.apply(v);
            let at_reference = ratio(reference);
            for v in [reference / 2.0, 1e-4, 1e-6] {
                assert!((ratio(v) / at_reference - 1.0).abs() < 1e-5, "at {v}");
            }
            // And the scale's limit at Y ≤ 0 is that same factor on reinhard's gain.
            assert!((o.scale(0.0) / o.scale(1e-9) - 1.0).abs() < 1e-5);
            assert!((o.scale(-0.01) / o.scale(1e-9) - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn the_dark_end_is_monotonic_and_finite() {
        let check = |p: FitRangeParams, what: String| {
            let o = operator(&p);
            let mut last = 0.0;
            for v in sweep() {
                let y = o.apply(v);
                assert!(y.is_finite() && y >= last, "{what}: not monotonic at {v}");
                last = y;
            }
        };
        for base in [0.001_f32, CAPPED_BASE, 0.05, 0.17] {
            check(
                with_black(DarkEnd::Black(12.0), base, 1.0),
                format!("base {base}"),
            );
        }
        // The floor at its deepest reach: a reference far under it would fold the curve
        // over without the cap.
        for dark in [1e-9_f32, 1e-6, 1e-4, STEEP_DARK, 0.01] {
            for stops in [0.5_f32, REVIEWED_DARK_FLOOR_STOPS, MAX_DARK_END_STOPS] {
                check(
                    with_floor(stops, Some(dark), 1.0),
                    format!("dark {dark}, floor {stops}"),
                );
            }
        }
    }

    #[test]
    fn the_floor_s_lift_is_capped_so_the_curve_stays_at_half_slope() {
        // ≈ 12 stops under mid-grey: the 4.5-stop floor would ask a 7.5-stop lift.
        let deep = 0.18 * 2f32.powi(-12) / 1.2194;
        let p = with_floor(REVIEWED_DARK_FLOOR_STOPS, Some(deep), 1.0);
        let r = p.resolved().dark_end;
        let dark = r.frame_dark_stops.unwrap();
        assert!(
            (r.shift_stops / dark - MAX_LIFT_SHARE as f32).abs() < 1e-5,
            "{r:?}"
        );
        // Held short, and said so; a floor reached is not.
        let w = r.warning().expect("warned");
        assert!(w.contains("cap") && w.contains("--contrast"), "{w}");
        let reached = with_floor(REVIEWED_DARK_FLOOR_STOPS, Some(STEEP_DARK), 1.0).resolved();
        assert_eq!(reached.dark_end.warning(), None);
        // The default target's lift is capped and warned about the same way, naming it.
        let t = with_target(DEFAULT_DARK_TARGET_STOPS, deep)
            .resolved()
            .dark_end;
        assert!((t.shift_stops - r.shift_stops).abs() < 1e-6, "{t:?}");
        let w = t.warning().expect("warned");
        assert!(w.contains("dark target's raise stops at its cap"), "{w}");
        assert!(w.contains("below the dark target's 5.5"), "{w}");
        let o = operator(&p);
        let slope = |v: f32| (o.apply(v * 1.001) / o.apply(v)).ln() / 1.001f32.ln();
        let flattest = sweep()
            .filter(|&v| v > deep && v < 0.17)
            .map(slope)
            .fold(f32::INFINITY, f32::min);
        assert!(flattest > 0.49, "{flattest}");
    }

    #[test]
    fn a_film_base_already_at_or_below_the_target_is_left_alone() {
        let plain = op(DEFAULT_HEADROOM_STOPS, 1.0);
        // 0.0025 renders ≈ 6.0 stops under mid-grey: below a 5-stop target, above 7.
        for (stops, left_alone) in [(5.0, true), (7.0, false)] {
            let p = with_black(DarkEnd::Black(stops), 0.0025, 1.0);
            let r = p.resolved().dark_end;
            assert_eq!(r.curve == IDENTITY, left_alone, "{stops}: {r:?}");
            assert_eq!(r.shift_stops == 0.0, left_alone, "{stops}: {r:?}");
            if left_alone {
                for v in sweep() {
                    assert_eq!(operator(&p).apply(v).to_bits(), plain.apply(v).to_bits());
                }
            }
        }
        let off = with_black(DarkEnd::Off, CAPPED_BASE, 1.0).resolved();
        assert_eq!(off.dark_end.curve, IDENTITY);
        assert_eq!(off.dark_end.shift_stops, 0.0);
    }

    #[test]
    fn the_dark_end_agrees_across_peaks_below_white() {
        for (sdr, hdr) in [
            (
                with_black(BLACK, CAPPED_BASE, 1.0),
                with_black(BLACK, CAPPED_BASE, HDR_PEAK),
            ),
            (
                with_floor(REVIEWED_DARK_FLOOR_STOPS, Some(STEEP_DARK), 1.0),
                with_floor(REVIEWED_DARK_FLOOR_STOPS, Some(STEEP_DARK), HDR_PEAK),
            ),
        ] {
            let (sdr, hdr) = (operator(&sdr), operator(&hdr));
            for v in sweep().filter(|v| *v <= DIFFUSE_WHITE) {
                assert_eq!(sdr.apply(v).to_bits(), hdr.apply(v).to_bits(), "at {v}");
            }
        }
    }

    #[test]
    fn a_film_base_near_or_above_mid_grey_is_warned_about() {
        // 1.9 stops under: black runs, squeezed into the band — warned.
        let near = 0.18 * 2f32.powf(-1.9) / 1.2194;
        let r = with_black(BLACK, near, 1.0).resolved().dark_end;
        assert_eq!(r.curve, DARK_END_CURVE, "{r:?}");
        let w = r.warning().expect("warned");
        assert!(w.contains("crushing") && w.contains("--exposure"), "{w}");
        assert!(w.contains("--display-black off"), "{w}");
        // At mid-grey and above: skipped — warned, not silent.
        for base in [0.18_f32, 0.5] {
            let r = with_black(BLACK, base, 1.0).resolved().dark_end;
            assert_eq!(r.curve, IDENTITY, "{r:?}");
            assert!(r.warning().expect("warned").contains("skipped"));
        }
        // A normal base, black off, and the floor and target (which never read the base),
        // are not.
        let r = with_black(BLACK, CAPPED_BASE, 1.0).resolved().dark_end;
        assert_eq!(r.warning(), None);
        for setting in [
            DarkEnd::Off,
            DarkEnd::Floor(REVIEWED_DARK_FLOOR_STOPS),
            DarkEnd::default(),
        ] {
            let r = with_black(setting, 0.5, 1.0).resolved().dark_end;
            assert_eq!(r.warning(), None);
        }
    }

    #[test]
    fn the_report_names_the_dark_end_when_only_it_runs() {
        assert_eq!(
            OPERATOR_AND_DARK_END,
            format!("{OPERATOR}+{DARK_END_CURVE}")
        );
        let mut p = with_black(BLACK, CAPPED_BASE, 1.0);
        assert_eq!(p.resolved().applied(), OPERATOR_AND_DARK_END);
        p.dark_end = DarkEnd::Off;
        assert_eq!(p.resolved().applied(), OPERATOR);
        p.dark_end = BLACK;
        p.headroom_stops = 0.0;
        assert_eq!(p.resolved().applied(), DARK_END_CURVE);
        p.dark_end = DarkEnd::Off;
        assert_eq!(p.resolved().applied(), IDENTITY);
    }

    #[test]
    fn the_dark_end_reads_its_spellings() {
        assert_eq!(DarkEnd::parse_floor("4.5"), Ok(DarkEnd::Floor(4.5)));
        assert_eq!(DarkEnd::parse_black("6"), Ok(DarkEnd::Black(6.0)));
        assert_eq!(DarkEnd::parse_target("5.5"), Ok(DarkEnd::Target(5.5)));
        assert_eq!(DarkEnd::parse_floor("off"), Ok(DarkEnd::Off));
        assert_eq!(DarkEnd::parse_target("off"), Ok(DarkEnd::Off));
        assert_eq!(DarkEnd::parse_black("off"), Ok(DarkEnd::Off));
        assert!(DarkEnd::parse_floor("none").is_err());
        for (json, value) in [
            (r#"{"floor": 4.5}"#, DarkEnd::Floor(4.5)),
            (r#"{"black": 6}"#, DarkEnd::Black(6.0)),
            (r#"{"target": 5.5}"#, DarkEnd::Target(5.5)),
            (r#""off""#, DarkEnd::Off),
        ] {
            let back: DarkEnd = serde_json::from_str(json).unwrap();
            assert_eq!(back, value);
            assert_eq!(
                serde_json::from_str::<DarkEnd>(&serde_json::to_string(&value).unwrap()).unwrap(),
                value
            );
        }
        for bad in [
            r#""none""#,
            "true",
            "6",
            r#"{"floor": 4, "black": 6}"#,
            r#"{"floor": "off"}"#,
        ] {
            assert!(serde_json::from_str::<DarkEnd>(bad).is_err(), "{bad}");
        }
        for bad in [0.0, -1.0, 16.5, f32::NAN, f32::INFINITY] {
            for setting in [
                DarkEnd::Floor(bad),
                DarkEnd::Target(bad),
                DarkEnd::Black(bad),
            ] {
                let Err(DarkEndFault(v)) = setting.check() else {
                    panic!("{setting:?} accepted");
                };
                assert_eq!(v.to_bits(), bad.to_bits());
            }
        }
        assert_eq!(DarkEnd::Black(MAX_DARK_END_STOPS).check(), Ok(()));
        assert_eq!(DarkEnd::Off.check(), Ok(()));
        assert_eq!(
            DarkEnd::default(),
            DarkEnd::Target(DEFAULT_DARK_TARGET_STOPS)
        );
    }

    #[test]
    fn the_report_names_the_operator_only_when_it_runs() {
        let params = |stops| FitRangeParams {
            headroom_stops: stops,
            peak: DisplayPeak::SDR,
            dark_end: DarkEnd::Off,
            film_base: 0.0138,
            frame_dark: None,
        };
        let r = params(DEFAULT_HEADROOM_STOPS).resolved();
        assert_eq!(r.operator, OPERATOR);
        assert_eq!(r.white_point, 64.0);
        assert_eq!(r.display_peak, DisplayPeak::SDR);
        assert_eq!(params(0.0).resolved().operator, "identity");
    }
}
