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
//!   **Display black is not**: it calls `log2` and `exp2` per frame and per pixel below
//!   mid-grey (see below), so with it on — the default — output below mid-grey may
//!   differ by a ULP across targets. From mid-grey up, and with black off, SDR output
//!   is bit-reproducible at a whole-stop headroom.
//! - **Below diffuse white every peak agrees exactly**, because the lift is zero there
//!   and `r` is shared. A gain map needs the two renditions to agree below diffuse
//!   white, and here that is a property of the formula rather than a tolerance.
//! - **Mid-grey is preserved** (`r(0.18) = 0.18` at every `W`), and `headroom_stops = 0`
//!   makes reinhard the identity on every branch. The stage is the identity only with
//!   display black off too: at zero headroom black still places the film base.
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
//! # Display black
//!
//! **Reinhard compresses upward only**: below mid-grey it is nearly a gain (log-log
//! slope 1.00 at 0.002, 0.94 near 0.05), so without help black lands wherever the
//! decode and the look's contrast put it — the film base at L\* 7–9 on a roll at whole
//! contrast ≈ 2.25, pale. So the stage also places black, on reinhard's output `y` and
//! before the peak's lift:
//!
//! ```text
//! y′ = y · 2^(shift · (1 − smoothstep(u)))    u = (log2 y − log2 b) / (log2 0.18 − log2 b)
//! shift = log2(T / b)                          b = r(film base),  T = 0.18 · 2^−display_black
//! ```
//!
//! - **The reference is the film base, where it renders** (`b`): every frame's darkest
//!   content bottoms out there, and where it lands is decided by the frame's own
//!   parameters — above all the look's contrast — so it is the base run through the
//!   frame's grade (`chain::render` does that), never an image statistic. A frame
//!   clamped to a different contrast from its roll gets its own shift.
//! - **A shift in stops, whole at the base and below, none from mid-grey up.** Below the
//!   base the slope stays 1 (shadow texture is darker, not squeezed toward 0); the
//!   stops between the base and mid-grey take the extra contrast; mid-grey, white and
//!   everything the peak's lift touches are untouched, bit for bit.
//! - **Luminance only**, like the rest of the stage: the channels scale together, so a
//!   dark near-neutral keeps its chromaticity. A per-channel subtraction — the legacy
//!   `--black-point`, and the probe `nf-calibration/anchor-comparison` judged with —
//!   amplifies whatever tint the shadows carry.
//! - **A base already at or below the target is left alone**, never lifted
//!   (`nf-display-stages/parametric-operator`, reviewed 2026-09-26).
//! - **Below diffuse white every peak still agrees exactly**: black acts below
//!   mid-grey, where the lift is zero.
//!
//! This is where the base lands on the display, and the only black placement: there is
//! no scene-side flare/fog subtraction (`nf-scene-correction/flare-removal`, closed as
//! not needed).
//!
//! **Reinhard is kept on purpose** (`nf-display-stages/parametric-shoulder`,
//! 2026-09-27). With mid-grey, the rendered white and the peak all pinned, any smooth
//! shoulder stays within about 0.15 stop of it. A brighter white with mid-grey pinned
//! lost to it in review. Where white renders is the look's contrast (the roll's white
//! rule). The headroom barely moves white (0.09 stop from 6 stops to 2); it sets how
//! hard the stops above white are compressed.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

/// Pinned identifier of display black's curve, for the report. A change to its pixels
/// moves the version.
pub const BLACK_CURVE: &str = "log-shift-to-mid-grey-v1";

/// What the stage list names when reinhard and display black both run — the default —
/// joined in the order they run, as the look joins its controls.
pub const OPERATOR_AND_BLACK: &str = "reinhard-peak-lifted-v1+log-shift-to-mid-grey-v1";

/// Display black's default: the film base 6 stops below mid-grey on the display
/// (L\* ≈ 2.5), chosen by review (`nf-display-stages/parametric-operator`).
pub const DEFAULT_DISPLAY_BLACK_STOPS: f32 = 6.0;

/// The deepest display black accepted, in stops below mid-grey: 16 stops puts the base
/// at `0.18 · 2^−16`, under a 16-bit encode's first code value.
pub const MAX_DISPLAY_BLACK_STOPS: f32 = 16.0;

/// Where the film base renders on the display — the recipe's `fit_range.display_black`,
/// `--display-black`: stops below mid-grey, or `"off"`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DisplayBlack {
    /// Black left where the decode and the look put it.
    Off,
    /// The base at `0.18 · 2^−stops` on the display.
    StopsBelowMid(f32),
}

impl Default for DisplayBlack {
    fn default() -> Self {
        DisplayBlack::StopsBelowMid(DEFAULT_DISPLAY_BLACK_STOPS)
    }
}

/// A [`DisplayBlack`] that is not finite, or not in `(0, MAX_DISPLAY_BLACK_STOPS]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayBlackFault(pub f32);

impl DisplayBlack {
    /// The value rule, rendered by the recipe gate as a usage error.
    pub fn check(self) -> std::result::Result<(), DisplayBlackFault> {
        match self {
            DisplayBlack::StopsBelowMid(v) if !(v > 0.0 && v <= MAX_DISPLAY_BLACK_STOPS) => {
                Err(DisplayBlackFault(v))
            }
            _ => Ok(()),
        }
    }

    /// The target's luminance on the display, when on.
    fn target(self) -> Option<f64> {
        match self {
            DisplayBlack::Off => None,
            DisplayBlack::StopsBelowMid(stops) => Some(MID_GREY * (-f64::from(stops)).exp2()),
        }
    }
}

/// The literal `"off"`, spelled once for the recipe, the flag and the report.
const OFF: &str = "off";

impl std::str::FromStr for DisplayBlack {
    type Err = String;

    /// The flag's spelling: a number of stops, or `off`. The range is the recipe
    /// gate's, so the flag and the key share one message.
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        if s == OFF {
            return Ok(DisplayBlack::Off);
        }
        s.parse::<f32>()
            .map(DisplayBlack::StopsBelowMid)
            .map_err(|_| format!("expected stops below mid-grey (e.g. 6) or `{OFF}`, got {s:?}"))
    }
}

impl Serialize for DisplayBlack {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            DisplayBlack::Off => s.serialize_str(OFF),
            DisplayBlack::StopsBelowMid(v) => s.serialize_f32(*v),
        }
    }
}

impl<'de> Deserialize<'de> for DisplayBlack {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = DisplayBlack;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "stops below mid-grey (a number) or \"{OFF}\"")
            }
            fn visit_str<E: serde::de::Error>(
                self,
                v: &str,
            ) -> std::result::Result<DisplayBlack, E> {
                if v == OFF {
                    Ok(DisplayBlack::Off)
                } else {
                    Err(E::invalid_value(serde::de::Unexpected::Str(v), &self))
                }
            }
            fn visit_f64<E: serde::de::Error>(
                self,
                v: f64,
            ) -> std::result::Result<DisplayBlack, E> {
                Ok(DisplayBlack::StopsBelowMid(v as f32))
            }
            fn visit_i64<E: serde::de::Error>(
                self,
                v: i64,
            ) -> std::result::Result<DisplayBlack, E> {
                Ok(DisplayBlack::StopsBelowMid(v as f32))
            }
            fn visit_u64<E: serde::de::Error>(
                self,
                v: u64,
            ) -> std::result::Result<DisplayBlack, E> {
                Ok(DisplayBlack::StopsBelowMid(v as f32))
            }
        }
        d.deserialize_any(Visitor)
    }
}

/// The display's peak, relative to diffuse white — the one argument the two display
/// branches differ in.
///
/// Checked, so an unusable peak cannot reach the operator: below `1.0` the lift would
/// *darken* above diffuse white, and a non-finite one poisons every bright pixel.
/// Bounded above by PQ's 10,000 nits over the 203-nit reference white, the brightest
/// signal any display destination can carry.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DisplayPeak(f32);

impl DisplayPeak {
    /// An SDR display: diffuse white is the peak.
    pub const SDR: Self = DisplayPeak(1.0);

    /// The largest representable peak: PQ's 10,000 nits over 203-nit reference white.
    const MAX: f32 = 10_000.0 / 203.0;

    /// Check a peak, or refuse it.
    pub fn new(peak: f32) -> Result<Self> {
        if !peak.is_finite() || !(1.0..=Self::MAX).contains(&peak) {
            return Err(NcError::Other(format!(
                "fit range's display peak must be finite and within [1, {}] times \
                 diffuse white, got {peak}",
                Self::MAX
            )));
        }
        Ok(DisplayPeak(peak))
    }

    /// The peak as a multiple of diffuse white.
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
    /// Where the film base renders on the display.
    pub display_black: DisplayBlack,
    /// The film base's **graded** ACEScg luminance — the decoded base run through the
    /// frame's own scene correction and look, which `chain::render` does. Display
    /// black's reference; read only when it is on.
    pub film_base: f32,
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
            // moved no pixel. `apply` skips the pixels only when display black is off
            // too; [`FitRange::applied`] joins the two for the stage list.
            operator: if white_point == 1.0 {
                IDENTITY
            } else {
                OPERATOR
            },
            headroom_stops: self.headroom_stops,
            white_point,
            display_peak: self.peak,
            display_black: self.black().0,
        }
    }

    /// Display black, resolved against where the film base renders without it: the
    /// report's account, and the shift `apply` runs (`None` when off or left alone).
    fn black(&self) -> (ResolvedBlack, Option<Shift>) {
        let white_point = self.white_point();
        let base = reinhard(
            self.film_base,
            white_point,
            mid_grey_preserving_gain(white_point),
        );
        let base = f64::from(base);
        let shift = self
            .display_black
            .target()
            .filter(|&target| base > target && base < MID_GREY)
            .map(|target| (target / base).log2())
            .map(|stops| Shift {
                stops,
                whole: stops.exp2(),
                base: base.log2(),
                knee: MID_GREY.log2(),
            });
        let resolved = ResolvedBlack {
            setting: self.display_black,
            curve: if shift.is_some() {
                BLACK_CURVE
            } else {
                IDENTITY
            },
            film_base_stops: (MID_GREY / base).log2() as f32,
            shift_stops: shift.map_or(0.0, |s| s.stops as f32),
        };
        (resolved, shift)
    }
}

/// What display black did to a frame, for the report.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ResolvedBlack {
    /// The knob as set: stops below mid-grey, or `"off"`.
    pub setting: DisplayBlack,
    /// [`BLACK_CURVE`], or `"identity"` when black is off, the base already renders at
    /// or below the target (left alone), or the base renders at or above mid-grey,
    /// where a shift that holds mid-grey cannot reach it ([`ResolvedBlack::warning`]).
    pub curve: &'static str,
    /// Where the film base renders without display black, in stops below mid-grey on
    /// the display.
    pub film_base_stops: f32,
    /// The shift applied at the base, in stops (negative darkens); `0` when left alone.
    pub shift_stops: f32,
}

/// How far under mid-grey the film base must render for display black to place it
/// well. The shift is spread between the base and mid-grey, so a base closer than this
/// packs the whole shift into a narrow band — at 2 stops under and the default target,
/// a 4-stop shift over 2 stops steepens the band's middle 4× — and at mid-grey or
/// above black cannot run at all. A normal exposure renders the base 3.8–5.8 stops
/// under; at the default contrast it takes about `--exposure 1.75` to get here.
pub const MIN_FILM_BASE_STOPS: f32 = 2.0;

impl ResolvedBlack {
    /// A warning when display black is on but the film base renders less than
    /// [`MIN_FILM_BASE_STOPS`] under mid-grey — crushed into a narrow band below it,
    /// or (at or above mid-grey) not placed at all. `None` otherwise.
    pub fn warning(&self) -> Option<String> {
        if self.setting == DisplayBlack::Off || self.film_base_stops >= MIN_FILM_BASE_STOPS {
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
             `--display-black off` (recipe `fit_range.display_black`) to leave black where \
             the grade puts it"
        ))
    }
}

/// What fit range applied to a frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct FitRange {
    /// [`OPERATOR`], or `"identity"` when the headroom leaves nothing to compress.
    pub operator: &'static str,
    pub headroom_stops: f32,
    /// Reinhard's white point, `2^headroom_stops`.
    pub white_point: f32,
    /// The display's peak, as a multiple of diffuse white.
    pub display_peak: DisplayPeak,
    /// Where the film base landed on the display.
    pub display_black: ResolvedBlack,
}

impl FitRange {
    /// The stage's entry in the report's list, and the tone curve an HDR destination
    /// names: what ran, joined by `+` in the order it ran — reinhard, then display
    /// black — or `"identity"` when neither moves a pixel (the tests `apply` skips on).
    pub fn applied(&self) -> &'static str {
        match (
            self.operator != IDENTITY,
            self.display_black.curve != IDENTITY,
        ) {
            (true, true) => OPERATOR_AND_BLACK,
            (true, false) => self.operator,
            (false, true) => self.display_black.curve,
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
    if params.display_black != DisplayBlack::Off
        && !(params.film_base.is_finite() && params.film_base > 0.0)
    {
        return Err(NcError::Other(format!(
            "fit range was handed a film base that graded to luminance {}; the decoded \
             base is positive and finite by construction",
            params.film_base
        )));
    }
    let mut buffer = image.into_buffer();
    let white_point = params.white_point();
    let black = params.black().1;
    let identity = white_point == 1.0 && black.is_none();
    let operator = Operator::new(white_point, params.headroom_stops, params.peak, black);
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
    black: Option<Shift>,
}

impl Operator {
    fn new(white_point: f32, headroom_stops: f32, peak: DisplayPeak, black: Option<Shift>) -> Self {
        Self {
            white_point,
            gain: mid_grey_preserving_gain(white_point),
            bottom_stops: DIFFUSE_WHITE.log2(),
            top_stops: headroom_stops,
            peak: peak.value(),
            black,
        }
    }

    /// The factor a pixel of this luminance is scaled by: `f(Y)/Y`.
    ///
    /// **At `Y ≤ 0` it is the curve's limit at black, the mid-grey gain** — not `1` —
    /// times display black's whole shift. `f` is a pure gain near zero (`f(Y)/Y → gain`,
    /// ≈1.22 at six stops, and the shift is a constant factor below the base), so
    /// leaving those pixels unscaled would step every channel where a saturated
    /// colour's luminance crosses zero. The limit keeps the scale continuous, and still
    /// clamps nothing.
    fn scale(self, luminance: f32) -> f32 {
        if luminance <= 0.0 {
            let whole = self.black.map_or(1.0, |s| s.whole);
            return (self.gain * whole) as f32;
        }
        self.apply(luminance) / luminance
    }

    /// `r(v) · (1 + (P − 1)·s(v))` for a positive luminance.
    fn apply(self, value: f32) -> f32 {
        let base = reinhard(value, self.white_point, self.gain);
        let base = match self.black {
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

/// Display black's shift, resolved once per frame: `stops` at and below `base`, fading
/// with a smoothstep in `log2` to nothing at `knee`, mid-grey. `base` and `knee` are
/// `log2` of display luminance.
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

    /// Fit range at the default headroom with display black set, for a film base whose
    /// graded luminance is `film_base`.
    fn with_black(display_black: DisplayBlack, film_base: f32, peak: f32) -> FitRangeParams {
        FitRangeParams {
            headroom_stops: DEFAULT_HEADROOM_STOPS,
            peak: DisplayPeak::new(peak).unwrap(),
            display_black,
            film_base,
        }
    }

    fn operator(p: &FitRangeParams) -> Operator {
        Operator::new(p.white_point(), p.headroom_stops, p.peak, p.black().1)
    }

    /// A capped roll's film base: it renders ≈ 4.4 stops under mid-grey without black.
    const CAPPED_BASE: f32 = 0.0075;

    #[test]
    fn display_black_lands_the_film_base_on_its_target() {
        for stops in [4.5_f32, 6.0, 7.5] {
            let p = with_black(DisplayBlack::StopsBelowMid(stops), CAPPED_BASE, 1.0);
            let got = f64::from(operator(&p).apply(CAPPED_BASE));
            let want = MID_GREY * (-f64::from(stops)).exp2();
            assert!((got / want - 1.0).abs() < 1e-5, "{stops}: {got} vs {want}");
            let r = p.resolved().display_black;
            assert_eq!(r.curve, BLACK_CURVE);
            // The shift darkens, so the base ends up `film_base_stops − shift_stops` below mid.
            assert!(
                (r.film_base_stops - r.shift_stops - stops).abs() < 1e-4,
                "{r:?}"
            );
        }
    }

    #[test]
    fn display_black_leaves_mid_grey_and_everything_above_it_untouched() {
        let plain = op(DEFAULT_HEADROOM_STOPS, 1.0);
        let black = operator(&with_black(DisplayBlack::default(), CAPPED_BASE, 1.0));
        // Reinhard maps mid-grey to itself, so every input from 0.18 up renders at or
        // above the knee.
        for v in sweep().filter(|v| *v >= 0.18) {
            assert_eq!(black.apply(v).to_bits(), plain.apply(v).to_bits(), "at {v}");
        }
        assert!(black.apply(0.1) < plain.apply(0.1), "a shadow darkens");
    }

    #[test]
    fn display_black_keeps_slope_one_below_the_film_base() {
        let plain = op(DEFAULT_HEADROOM_STOPS, 1.0);
        let black = operator(&with_black(DisplayBlack::default(), CAPPED_BASE, 1.0));
        let ratio = |v: f32| black.apply(v) / plain.apply(v);
        let at_base = ratio(CAPPED_BASE);
        for v in [CAPPED_BASE / 2.0, 1e-4, 1e-6] {
            assert!((ratio(v) / at_base - 1.0).abs() < 1e-5, "at {v}");
        }
        // And the scale's limit at Y ≤ 0 is that same factor on reinhard's gain.
        assert!((black.scale(0.0) / black.scale(1e-9) - 1.0).abs() < 1e-5);
        assert!((black.scale(-0.01) / black.scale(1e-9) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn display_black_is_monotonic_and_finite() {
        for base in [0.001_f32, CAPPED_BASE, 0.05, 0.17] {
            let black = operator(&with_black(DisplayBlack::StopsBelowMid(12.0), base, 1.0));
            let mut last = 0.0;
            for v in sweep() {
                let y = black.apply(v);
                assert!(
                    y.is_finite() && y >= last,
                    "base {base}: not monotonic at {v}"
                );
                last = y;
            }
        }
    }

    #[test]
    fn a_film_base_already_at_or_below_the_target_is_left_alone() {
        let plain = op(DEFAULT_HEADROOM_STOPS, 1.0);
        // 0.0025 renders ≈ 6.0 stops under mid-grey: below a 5-stop target, above 7.
        for (stops, left_alone) in [(5.0, true), (7.0, false)] {
            let p = with_black(DisplayBlack::StopsBelowMid(stops), 0.0025, 1.0);
            let r = p.resolved().display_black;
            assert_eq!(r.curve == IDENTITY, left_alone, "{stops}: {r:?}");
            assert_eq!(r.shift_stops == 0.0, left_alone, "{stops}: {r:?}");
            if left_alone {
                for v in sweep() {
                    assert_eq!(operator(&p).apply(v).to_bits(), plain.apply(v).to_bits());
                }
            }
        }
        let off = with_black(DisplayBlack::Off, CAPPED_BASE, 1.0).resolved();
        assert_eq!(off.display_black.curve, IDENTITY);
        assert_eq!(off.display_black.shift_stops, 0.0);
    }

    #[test]
    fn display_black_agrees_across_peaks_below_white() {
        let sdr = operator(&with_black(DisplayBlack::default(), CAPPED_BASE, 1.0));
        let hdr = operator(&with_black(DisplayBlack::default(), CAPPED_BASE, HDR_PEAK));
        for v in sweep().filter(|v| *v <= DIFFUSE_WHITE) {
            assert_eq!(sdr.apply(v).to_bits(), hdr.apply(v).to_bits(), "at {v}");
        }
    }

    #[test]
    fn a_film_base_near_or_above_mid_grey_is_warned_about() {
        // 1.9 stops under: black runs, squeezed into the band — warned.
        let near = 0.18 * 2f32.powf(-1.9) / 1.2194;
        let r = with_black(DisplayBlack::default(), near, 1.0)
            .resolved()
            .display_black;
        assert_eq!(r.curve, BLACK_CURVE, "{r:?}");
        let w = r.warning().expect("warned");
        assert!(w.contains("crushing") && w.contains("--exposure"), "{w}");
        assert!(w.contains("--display-black off"), "{w}");
        // At mid-grey and above: skipped — warned, not silent.
        for base in [0.18_f32, 0.5] {
            let r = with_black(DisplayBlack::default(), base, 1.0)
                .resolved()
                .display_black;
            assert_eq!(r.curve, IDENTITY, "{r:?}");
            assert!(r.warning().expect("warned").contains("skipped"));
        }
        // A normal base, and black off, are not.
        let r = with_black(DisplayBlack::default(), CAPPED_BASE, 1.0)
            .resolved()
            .display_black;
        assert_eq!(r.warning(), None);
        let r = with_black(DisplayBlack::Off, 0.5, 1.0)
            .resolved()
            .display_black;
        assert_eq!(r.warning(), None);
    }

    #[test]
    fn the_report_names_display_black_when_only_it_runs() {
        assert_eq!(OPERATOR_AND_BLACK, format!("{OPERATOR}+{BLACK_CURVE}"));
        let mut p = with_black(DisplayBlack::default(), CAPPED_BASE, 1.0);
        assert_eq!(p.resolved().applied(), OPERATOR_AND_BLACK);
        p.display_black = DisplayBlack::Off;
        assert_eq!(p.resolved().applied(), OPERATOR);
        p.display_black = DisplayBlack::default();
        p.headroom_stops = 0.0;
        assert_eq!(p.resolved().applied(), BLACK_CURVE);
        p.display_black = DisplayBlack::Off;
        assert_eq!(p.resolved().applied(), IDENTITY);
    }

    #[test]
    fn display_black_reads_stops_or_off() {
        assert_eq!(
            "6".parse::<DisplayBlack>(),
            Ok(DisplayBlack::StopsBelowMid(6.0))
        );
        assert_eq!("off".parse::<DisplayBlack>(), Ok(DisplayBlack::Off));
        assert!("none".parse::<DisplayBlack>().is_err());
        for (json, value) in [
            ("6.5", DisplayBlack::StopsBelowMid(6.5)),
            ("6", DisplayBlack::StopsBelowMid(6.0)),
            ("\"off\"", DisplayBlack::Off),
        ] {
            let back: DisplayBlack = serde_json::from_str(json).unwrap();
            assert_eq!(back, value);
            assert_eq!(
                serde_json::from_str::<DisplayBlack>(&serde_json::to_string(&value).unwrap())
                    .unwrap(),
                value
            );
        }
        assert!(serde_json::from_str::<DisplayBlack>("\"none\"").is_err());
        assert!(serde_json::from_str::<DisplayBlack>("true").is_err());
        for bad in [0.0, -1.0, 16.5, f32::NAN, f32::INFINITY] {
            let Err(DisplayBlackFault(v)) = DisplayBlack::StopsBelowMid(bad).check() else {
                panic!("{bad} accepted");
            };
            assert_eq!(v.to_bits(), bad.to_bits());
        }
        assert_eq!(
            DisplayBlack::StopsBelowMid(MAX_DISPLAY_BLACK_STOPS).check(),
            Ok(())
        );
        assert_eq!(DisplayBlack::Off.check(), Ok(()));
    }

    #[test]
    fn the_report_names_the_operator_only_when_it_runs() {
        let params = |stops| FitRangeParams {
            headroom_stops: stops,
            peak: DisplayPeak::SDR,
            display_black: DisplayBlack::Off,
            film_base: 0.0138,
        };
        let r = params(DEFAULT_HEADROOM_STOPS).resolved();
        assert_eq!(r.operator, OPERATOR);
        assert_eq!(r.white_point, 64.0);
        assert_eq!(r.display_peak, DisplayPeak::SDR);
        assert_eq!(params(0.0).resolved().operator, "identity");
    }
}
