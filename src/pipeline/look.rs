//! **Stage 2 of the new rendering chain — the look.**
//!
//! The creative stage: print contrast and highlight desaturation today, and later
//! the per-channel grade with a mid-grey pivot, print emulation and per-stock
//! normalization. Scene-referred and linear. Each control lands here
//! with its own task under `nf-look`, as its own key in the recipe's `look`
//! section — not one CDL-style object, whose slope and offset would duplicate white
//! balance and the flare subtraction, which scene correction owns.
//!
//! **Its position is a constraint, not a preference.** The look sits after scene
//! correction and *above* the SDR/HDR branch, because a gain map requires the two
//! renditions to agree below diffuse white — so anything shaping midtone character
//! must be applied once, before the split (`docs/design-update.md` Part 2). Only
//! fit range and later may differ per branch.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::algo::fixed::DIFFUSE_WHITE;
use crate::pipeline::colorimetry::dot;
use crate::pipeline::colorimetry::pinned::ACESCG_LUMA;
use crate::pipeline::pixels;
use crate::pipeline::scene_correction::SceneReferredImage;
use crate::pipeline::working_image::WorkingBuffer;
use crate::types::{NcError, Result};

/// **Highlight desaturation** (`nf-look/path-to-white`), `look.highlight_desaturation`:
/// chroma pulled toward the neutral axis as a pixel approaches diffuse white — the
/// path to white, made a deliberate control.
///
/// ```text
/// b  = smoothstep in stops: 0 at `start_stops` → 1 at diffuse white, held above
/// w  = 1 at s ≤ band[0] → 0 at s ≥ band[1], linear in s,
///      s = log10(max/min) / (linearization · contrast), over ACEScg channels
/// rgb ← rgb + strength · b · w · (Y − rgb)            Y = ACEScg luminance
/// ```
///
/// - **Keyed on distance from the neutral axis as well as brightness**, or a bright
///   *coloured* surface is neutralised as hard as a bright white one — the sunset case
///   `docs/spike/highlight-desaturation.md` found on one marked patch. `s` is the
///   negative's own density spread — the log ratio over **the whole contrast that
///   shaped the pixel**, the decode's linearization times [`LookSection::contrast`],
///   which runs first — so the band means the same on a flat roll and a contrasty one,
///   and the same whether a roll's contrast is stated in the decode or in the look.
/// - **The band assumes a roll-level white balance ahead of it**
///   (`hanten measure-roll`): `s` measures distance from R = G = B, which is distance
///   from white only once the roll's cast is gone (`docs/spike/desaturation-band.md`).
/// - **Anchored at diffuse white**, [`DIFFUSE_WHITE`], before the SDR/HDR branch, so
///   both renditions inherit the same convergence. It is a highlight operator and
///   reaches nothing below `start_stops`.
/// - **Luminance is kept**: the pull is a straight line to `(Y, Y, Y)`.
/// - **On by default at `0.8`** (user decision 2026-09-24): visible on the frames it
///   is for (bright near-white surfaces carrying a residual cast) and invisible on the
///   rest. **`strength = 0` is off**, a bit-exact identity.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HighlightDesaturation {
    /// How far a fully-keyed pixel moves toward neutral, in `[0, 1]`.
    pub strength: f32,
    /// Where the pull starts, in stops relative to diffuse white (negative).
    pub start_stops: f32,
    /// `[s0, s1]`: full pull at or below `s0`, none at or above `s1`.
    pub band: [f32; 2],
}

impl Default for HighlightDesaturation {
    /// Strength `0.8`, one stop below white, band `0.015 → 0.025` on the ACEScg measure —
    /// where marked colours keep 92–100% of their chroma (`nf-look` progress,
    /// `path-to-white`).
    fn default() -> Self {
        Self {
            strength: 0.8,
            start_stops: -1.0,
            band: [0.015, 0.025],
        }
    }
}

/// Most stops below diffuse white the pull may start. Past this it is no longer a
/// *highlight* operator — midtone cast is the grade's (`nf-look/per-channel-grade`).
pub const MAX_START_STOPS: f32 = 8.0;

/// A value [`HighlightDesaturation::check`] refuses, carried as data so each caller
/// can name the knob the way its command spells it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DesaturationFault {
    Strength(f32),
    Start(f32),
    Band([f32; 2]),
}

impl HighlightDesaturation {
    /// The value rules.
    pub fn check(&self) -> std::result::Result<(), DesaturationFault> {
        if !(0.0..=1.0).contains(&self.strength) {
            return Err(DesaturationFault::Strength(self.strength));
        }
        if !(-MAX_START_STOPS..0.0).contains(&self.start_stops) {
            return Err(DesaturationFault::Start(self.start_stops));
        }
        let [s0, s1] = self.band;
        if !(s0.is_finite() && s1.is_finite() && 0.0 <= s0 && s0 < s1) {
            return Err(DesaturationFault::Band(self.band));
        }
        Ok(())
    }

    /// Whether it moves any pixel.
    pub fn is_off(&self) -> bool {
        self.strength == 0.0
    }
}

/// Mid-grey, the pivot [`LookSection::contrast`] turns about: the value the decode's
/// anchor rule pins a correctly exposed grey card at, so changing contrast pivots the
/// picture instead of moving it.
pub const MID_GREY: f32 = 0.18;

/// The look's default print contrast: `BUNDLED_CONTRAST / LINEARIZATION`, the half of
/// the single `gamma` nc shipped before the split that was never the film's
/// linearization (`nf-reconstruction/gamma-split`). At it, a neutral renders where the
/// bundled decode rendered it. A starting point rather than a tuned value — the value is
/// `nf-look/contrast`'s and `nf-calibration`'s.
pub const DEFAULT_CONTRAST: f32 =
    crate::algo::fixed::BUNDLED_CONTRAST / crate::algo::fixed::LINEARIZATION;

/// A [`LookSection::contrast`] the value rule refuses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContrastFault(pub f32);

/// The recipe's `look` section — one key per control (`nf-look/stage`) — and what the
/// report echoes as resolved. Fields are in the order the stage applies them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LookSection {
    /// **Print contrast** (`look.contrast`, `--contrast`): the half of `gamma` that is a
    /// look rather than the film's linearization.
    ///
    /// ```text
    /// out_c = MID_GREY · (in_c / MID_GREY)^contrast      per ACEScg channel
    /// ```
    ///
    /// - **One exponent on every channel, pivoted at mid-grey.** For a neutral this is
    ///   exactly a steeper decode — `0.18 · (10^(L·(D′−A_L)) / 0.18)^k` is
    ///   `10^(L·k·(D′−A_{Lk}))` — so the look's default reproduces the bundled decode's
    ///   neutrals. It is **not** the same operator on colour: it acts after the
    ///   NC film RGB v1 3×3, the decode's slope before it, and a power does not commute
    ///   with a matrix that mixes channels. Saturated colour therefore differs slightly
    ///   from the bundled decode, by design.
    /// - **Neutral stays neutral**, so a white the roll's white balance made neutral
    ///   stays neutral under any contrast (`pipeline::roll_white`).
    /// - **Exposure is in scene stops, and contrast expands it.** Scene correction runs
    ///   first, so a gain `e` becomes `e^contrast` in the output: `--exposure 1` at
    ///   contrast 1.11 moves the picture 1.11 stops. Exposure adjusts the scene after
    ///   reconstruction; contrast then expands everything about mid-grey, as paper
    ///   contrast does to a printing exposure.
    /// - **Runs before highlight desaturation**, which divides its saturation measure by
    ///   the whole contrast and keys on diffuse white in the graded image
    ///   ([`crate::algo::fixed::DIFFUSE_WHITE`]).
    /// - **A sample at or below zero, or non-finite, passes through**: a power is
    ///   defined on positive values, and a wide-gamut linear space holds negative ones.
    ///   Monotone either way.
    ///
    /// `1` is the identity, bit-exact.
    pub contrast: f32,
    pub highlight_desaturation: HighlightDesaturation,
}

impl Default for LookSection {
    fn default() -> Self {
        Self {
            contrast: DEFAULT_CONTRAST,
            highlight_desaturation: HighlightDesaturation::default(),
        }
    }
}

impl LookSection {
    /// The contrast's value rule: finite and positive — zero flattens every channel to
    /// mid-grey, and a negative exponent reverses the tone scale.
    pub fn check_contrast(&self) -> std::result::Result<(), ContrastFault> {
        if self.contrast.is_finite() && self.contrast > 0.0 {
            Ok(())
        } else {
            Err(ContrastFault(self.contrast))
        }
    }

    /// Whether the look moves no pixel — what the report's `applied` reads.
    pub fn is_empty(&self) -> bool {
        self.contrast == 1.0 && self.highlight_desaturation.is_off()
    }

    /// Whether the user asked for a look — neither the default nor an empty one. The one
    /// predicate a destination that runs no look (`film-master`) reads to refuse, rather
    /// than one rule per knob (`nf-look/stage`; the refusal is
    /// `nf-destinations/preset-set`'s). The default is spared because every default
    /// recipe carries it; an empty look because it renders exactly what such a
    /// destination does, and refusing it would kill the flags-win reset.
    #[cfg_attr(not(test), allow(dead_code))] // the film-master refusal (`nf-destinations/preset-set`)
    pub fn asks_for_a_look(&self) -> bool {
        !self.is_empty() && *self != Self::default()
    }
}

/// The look's parameters: the recipe's `look` section plus the decode's linearization,
/// which the saturation measure is normalised by. **No `Default`**, for the reason
/// `FitRangeParams` has none: the linearization is the decode's to state, and
/// `Recipe::chain_params` adds it.
#[derive(Clone, Debug, PartialEq)]
pub struct LookParams {
    pub section: LookSection,
    /// The decode's linearization (`reconstruction.linearization`).
    pub linearization: f32,
}

impl LookParams {
    /// **Test fixture**: an empty look (contrast 1, strength 0) at the fixed decode's
    /// default linearization — for tests that are not about the look.
    #[cfg(test)]
    pub fn off() -> Self {
        let mut section = LookSection {
            contrast: 1.0,
            ..LookSection::default()
        };
        section.highlight_desaturation.strength = 0.0;
        Self {
            section,
            linearization: crate::algo::fixed::LINEARIZATION,
        }
    }

    /// See [`LookSection::is_empty`].
    pub fn is_empty(&self) -> bool {
        self.section.is_empty()
    }

    /// What the look does under these parameters, for the report — stated by the
    /// stage rather than by its caller, so filling the stage changes the report in the
    /// same edit.
    pub fn applied(&self) -> &'static str {
        let contrast = self.section.contrast != 1.0;
        let desaturation = !self.section.highlight_desaturation.is_off();
        match (contrast, desaturation) {
            (false, false) => "identity",
            (true, false) => "contrast",
            (false, true) => "highlight-desaturation",
            (true, true) => "contrast+highlight-desaturation",
        }
    }
}

/// Linear ACEScg after the look: scene-referred, graded.
///
/// This is also **the one source both display branches share**: the branch point is
/// here (`pipeline::chain`'s module docs state the contract).
pub struct GradedImage(WorkingBuffer);

impl GradedImage {
    /// A second copy for the other display branch — a full-frame allocation, made once
    /// by `chain::render_pair`. Not `Clone`, so no caller outside `pipeline` can add a
    /// buffer the memory model does not count.
    #[cfg_attr(not(test), allow(dead_code))] // the gain-map destination (`nf-destinations/preset-set`)
    pub(in crate::pipeline) fn split(&self) -> GradedImage {
        GradedImage(self.0.copy())
    }

    /// Hand the buffer to the next stage. Consuming, so the pixels move rather
    /// than copy.
    pub(in crate::pipeline) fn into_buffer(self) -> WorkingBuffer {
        self.0
    }
}

impl fmt::Debug for GradedImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_named(f, "GradedImage")
    }
}

/// Apply the look: contrast, then highlight desaturation, in one pass. An empty look
/// returns the buffer untouched, bit for bit.
///
/// Nothing is clamped, and a non-finite sample passes through untouched for fit range
/// to refuse by name.
pub fn apply(image: SceneReferredImage, params: &LookParams) -> Result<GradedImage> {
    let mut buffer = image.into_buffer();
    if !params.is_empty() {
        let section = &params.section;
        if section.check_contrast().is_err() {
            return Err(NcError::Other(format!(
                "the look was handed an unusable contrast ({}); the recipe's validation \
                 should have refused it",
                section.contrast
            )));
        }
        let contrast = (section.contrast != 1.0).then_some(section.contrast);
        let pull = if section.highlight_desaturation.is_off() {
            None
        } else {
            Some(Pull::new(
                &section.highlight_desaturation,
                params.linearization * section.contrast,
            )?)
        };
        pixels::map_in_place(buffer.rgb_mut(), |px| {
            if let Some(k) = contrast {
                apply_contrast(px, k);
            }
            if let Some(pull) = &pull {
                pull.apply(px);
            }
        });
    }
    Ok(GradedImage(buffer))
}

/// [`LookSection::contrast`] on one pixel.
fn apply_contrast(px: &mut [f32; 3], contrast: f32) {
    for channel in px.iter_mut() {
        if channel.is_finite() && *channel > 0.0 {
            *channel = MID_GREY * (*channel / MID_GREY).powf(contrast);
        }
    }
}

/// Highlight desaturation with its per-frame constants resolved once.
#[derive(Clone, Copy, Debug)]
struct Pull {
    strength: f32,
    start_stops: f32,
    /// Luminance where the pull starts, `DIFFUSE_WHITE · 2^start_stops`.
    start_luminance: f32,
    /// The band edges as max/min ratios, `10^(contrast · s)`: a pixel outside the ramp
    /// is classified without a logarithm.
    ratio_full: f32,
    ratio_none: f32,
    band: [f32; 2],
    /// The whole contrast that shaped the pixel: the decode's linearization times the
    /// look's contrast, which has already run.
    contrast: f32,
}

impl Pull {
    fn new(p: &HighlightDesaturation, contrast: f32) -> Result<Self> {
        if p.check().is_err() || !(contrast.is_finite() && contrast > 0.0) {
            return Err(NcError::Other(format!(
                "highlight desaturation was handed unusable parameters ({p:?} at total \
                 contrast {contrast}); the recipe's validation should have refused them"
            )));
        }
        Ok(Self {
            strength: p.strength,
            start_stops: p.start_stops,
            start_luminance: DIFFUSE_WHITE * p.start_stops.exp2(),
            ratio_full: 10f32.powf(contrast * p.band[0]),
            ratio_none: 10f32.powf(contrast * p.band[1]),
            band: p.band,
            contrast,
        })
    }

    fn apply(&self, px: &mut [f32; 3]) {
        let y = dot(*px, ACESCG_LUMA);
        // A NaN luminance passes through untouched, for fit range to refuse.
        if y.is_nan() || y <= self.start_luminance {
            return;
        }
        let max = px[0].max(px[1]).max(px[2]);
        let min = px[0].min(px[1]).min(px[2]);
        // A channel at or below zero is further from neutral than any band edge; an
        // infinite one passes through for fit range to refuse (`inf / inf` is NaN).
        if min.is_nan() || min <= 0.0 || max.is_infinite() {
            return;
        }
        let ratio = max / min;
        if ratio >= self.ratio_none {
            return;
        }
        let key = if ratio <= self.ratio_full {
            1.0
        } else {
            let s = ratio.log10() / self.contrast;
            ((self.band[1] - s) / (self.band[1] - self.band[0])).clamp(0.0, 1.0)
        };
        let brightness = if y >= DIFFUSE_WHITE {
            1.0
        } else {
            let t = ((y / DIFFUSE_WHITE).log2() - self.start_stops) / -self.start_stops;
            let t = t.clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let a = self.strength * key * brightness;
        for channel in px.iter_mut() {
            *channel += a * (y - *channel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull(strength: f32) -> Pull {
        let p = HighlightDesaturation {
            strength,
            ..HighlightDesaturation::default()
        };
        Pull::new(&p, 2.0).unwrap()
    }

    fn luminance(px: [f32; 3]) -> f32 {
        dot(px, ACESCG_LUMA)
    }

    /// A pixel at `y` stops relative to diffuse white whose max/min ratio sits at
    /// `s` on the saturation measure (contrast 2): warm, red high and blue low.
    fn pixel(stops: f32, s: f32) -> [f32; 3] {
        let ratio = 10f32.powf(2.0 * s);
        let raw = [ratio.sqrt(), 1.0, 1.0 / ratio.sqrt()];
        let scale = DIFFUSE_WHITE * stops.exp2() / luminance(raw);
        raw.map(|v| v * scale)
    }

    fn chroma(px: [f32; 3]) -> f32 {
        let y = luminance(px);
        px.iter().map(|v| (v - y).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn off_is_a_bit_exact_identity_and_says_so() {
        let params = LookParams::off();
        assert!(params.is_empty());
        assert_eq!(params.applied(), "identity");
        let mut on = LookParams::off();
        on.section.highlight_desaturation.strength = 0.5;
        assert_eq!(on.applied(), "highlight-desaturation");
    }

    #[test]
    fn a_look_is_asked_for_only_when_it_is_neither_the_default_nor_empty() {
        let with = |f: fn(&mut LookSection)| {
            let mut section = LookSection::default();
            f(&mut section);
            section
        };
        // The default does something, yet asks for nothing.
        let default = LookSection::default();
        assert!(!default.is_empty() && !default.asks_for_a_look());
        // Empty is an identity however the pull's inert knobs sit: contrast 1 and
        // strength 0, both needed.
        let empty = |s: &mut LookSection| {
            s.contrast = 1.0;
            s.highlight_desaturation.strength = 0.0;
        };
        assert!(with(empty).is_empty() && !with(empty).asks_for_a_look());
        assert!(
            !with(|s| {
                s.contrast = 1.0;
                s.highlight_desaturation.strength = 0.0;
                s.highlight_desaturation.band = [0.02, 0.04];
                s.highlight_desaturation.start_stops = -2.0;
            })
            .asks_for_a_look()
        );
        // Either control alone, moved off its default, is a look.
        assert!(with(|s| s.highlight_desaturation.strength = 0.0).asks_for_a_look());
        assert!(with(|s| s.contrast = 1.0).asks_for_a_look());
        assert!(with(|s| s.contrast = 1.4).asks_for_a_look());
        assert!(with(|s| s.highlight_desaturation.strength = 0.5).asks_for_a_look());
        assert!(with(|s| s.highlight_desaturation.band = [0.02, 0.04]).asks_for_a_look());
    }

    #[test]
    fn a_near_white_neutral_desaturates_monotonically_and_keeps_its_luminance() {
        let px = pixel(0.0, 0.005);
        let mut last = chroma(px);
        for strength in [0.25, 0.5, 0.75, 1.0] {
            let mut out = px;
            pull(strength).apply(&mut out);
            let c = chroma(out);
            assert!(c < last, "strength {strength}: {c} !< {last}");
            last = c;
            let dy = (luminance(out) - luminance(px)).abs();
            assert!(
                dy <= 2.0 * f32::EPSILON * luminance(px),
                "luminance moved {dy}"
            );
        }
        assert!(
            last < 1e-6,
            "full strength at white reaches neutral: {last}"
        );
    }

    #[test]
    fn a_coloured_highlight_is_untouched() {
        // Above the band's top edge, at diffuse white and above.
        for stops in [0.0, 1.0] {
            let px = pixel(stops, 0.03);
            let mut out = px;
            pull(1.0).apply(&mut out);
            assert_eq!(out, px, "{stops} stops");
        }
    }

    #[test]
    fn it_reaches_nothing_below_its_start() {
        let px = pixel(-1.5, 0.0);
        let mut out = px;
        pull(1.0).apply(&mut out);
        assert_eq!(out, px);
    }

    #[test]
    fn the_band_ramp_is_continuous_at_both_edges() {
        let [s0, s1] = HighlightDesaturation::default().band;
        let moved = |s: f32| {
            let px = pixel(0.0, s);
            let mut out = px;
            pull(1.0).apply(&mut out);
            1.0 - chroma(out) / chroma(px)
        };
        assert!((moved(s0 * 0.999) - 1.0).abs() < 1e-3);
        assert!((moved(s0 * 1.001) - 1.0).abs() < 1e-2);
        assert!(moved(s1 * 0.999) < 1e-2);
        assert_eq!(moved(s1 * 1.001), 0.0);
        let mid = moved((s0 + s1) / 2.0);
        assert!((mid - 0.5).abs() < 0.02, "{mid}");
    }

    #[test]
    fn the_measure_is_the_negatives_spread_whatever_the_contrast() {
        // One density spread rendered at two contrasts: the ratio differs, the key
        // does not.
        let spread = 0.02;
        let at = |contrast: f32| {
            let ratio = 10f32.powf(contrast * spread);
            let raw = [ratio.sqrt(), 1.0, 1.0 / ratio.sqrt()];
            let scale = DIFFUSE_WHITE / luminance(raw);
            let px = raw.map(|v| v * scale);
            let p = HighlightDesaturation {
                strength: 1.0,
                ..HighlightDesaturation::default()
            };
            let mut out = px;
            Pull::new(&p, contrast).unwrap().apply(&mut out);
            1.0 - chroma(out) / chroma(px)
        };
        assert!(
            (at(2.0) - at(4.0)).abs() < 1e-3,
            "{} vs {}",
            at(2.0),
            at(4.0)
        );
    }

    #[test]
    fn a_non_finite_or_non_positive_sample_passes_through() {
        for px in [
            [f32::NAN, 1.0, 1.0],
            [f32::INFINITY, 1.0, 1.0],
            [f32::INFINITY; 3],
            [1.2, 1.0, 0.0],
            [1.2, 1.0, -0.1],
        ] {
            let mut out = px;
            pull(1.0).apply(&mut out);
            assert_eq!(out.map(f32::to_bits), px.map(f32::to_bits), "{px:?}");
        }
    }

    // --- contrast ---------------------------------------------------------------

    /// A scan through the fixed decode at `linearization`, the NC film RGB v1 mapping,
    /// an identity scene correction and the look `section` — the chain up to the
    /// graded image, so the split can be compared against the bundled decode.
    fn graded(scan: &[f32], linearization: f32, section: LookSection) -> Vec<f32> {
        graded_with(scan, linearization, linearization, section)
    }

    /// [`graded`], with the look told a linearization other than the decode's — how a
    /// test reaches a wrong saturation divisor through the stage itself.
    fn graded_with(
        scan: &[f32],
        linearization: f32,
        look_linearization: f32,
        section: LookSection,
    ) -> Vec<f32> {
        use crate::algo::fixed::{self, DecodeParams};
        use crate::pipeline::scene_correction::{self, SceneCorrectionParams};
        use crate::pipeline::working_space::map_nc_film_rgb_v1;
        use crate::types::{FilmBase, LinearImage};
        let n = (scan.len() / 3) as u32;
        let image = LinearImage::new(n, 1, scan.to_vec(), None).unwrap();
        let params = DecodeParams {
            linearization,
            ..DecodeParams::default()
        };
        let (film, _) = fixed::decode(&image, &FilmBase::from(BASE), &params).unwrap();
        let (corrected, _) =
            scene_correction::apply(map_nc_film_rgb_v1(film), &SceneCorrectionParams::default())
                .unwrap();
        let params = LookParams {
            section,
            linearization: look_linearization,
        };
        apply(corrected, &params)
            .unwrap()
            .into_buffer()
            .into_linear()
            .rgb
    }

    const BASE: [f32; 3] = [0.9, 0.55, 0.42];

    /// Scan values whose corrected densities `D′_c` are `per_channel(t)` for each `t`.
    fn scan_at(ts: &[f32], per_channel: impl Fn(f32) -> [f32; 3]) -> Vec<f32> {
        let scale = crate::algo::fixed::DENSITY_SCALE;
        ts.iter()
            .flat_map(|&t| {
                let d = per_channel(t);
                (0..3).map(move |c| BASE[c] * 10f32.powf(-d[c] / scale[c]))
            })
            .collect()
    }

    /// The bundled decode's look: contrast carried by the decode, so the look's own is
    /// the identity.
    fn bundled_section(desaturation: HighlightDesaturation) -> LookSection {
        LookSection {
            contrast: 1.0,
            highlight_desaturation: desaturation,
        }
    }

    fn off_desaturation() -> HighlightDesaturation {
        HighlightDesaturation {
            strength: 0.0,
            ..HighlightDesaturation::default()
        }
    }

    const DENSITIES: [f32; 9] = [0.0, 0.1, 0.3, 0.5, 0.62, 0.8, 1.0, 1.3, 1.8];

    #[test]
    fn the_default_split_renders_a_neutral_where_the_bundled_decode_did() {
        // The task's acceptance check. For a neutral the look's pivoted power is a
        // steeper decode exactly — `0.18·(10^(L·(D′−A_L))/0.18)^k = 10^(Lk·(D′−A_Lk))` —
        // so the two agree up to f32 rounding: the product `1.8 · (2.0/1.8)` is not
        // exactly 2.0 in f32, and the split takes two `powf`s where the bundled decode
        // takes one. Measured at most 3.6e-7 relative (a few ULP) over the ramp, from
        // the film base to 1.8 density; bounded at 2e-6.
        use crate::algo::fixed::{BUNDLED_CONTRAST, LINEARIZATION};
        let scan = scan_at(&DENSITIES, |t| [t; 3]);
        let bundled = graded(&scan, BUNDLED_CONTRAST, bundled_section(off_desaturation()));
        let split = graded(
            &scan,
            LINEARIZATION,
            LookSection {
                highlight_desaturation: off_desaturation(),
                ..LookSection::default()
            },
        );
        for (i, (a, b)) in bundled.iter().zip(&split).enumerate() {
            let rel = ((a - b) / a).abs();
            assert!(rel < 2e-6, "sample {i}: bundled {a} vs split {b} ({rel:e})");
        }
    }

    #[test]
    fn the_default_split_moves_saturated_colour_by_design() {
        // Not the same operator on colour: the decode's slope acts before the NC film
        // RGB v1 3×3, the look's after it, and a power does not commute with a matrix
        // that mixes channels. Stated rather than hidden — the difference exists, and
        // it is small: 2.5% on the worst channel of a pixel ±0.25 density off neutral.
        use crate::algo::fixed::{BUNDLED_CONTRAST, LINEARIZATION};
        let scan = scan_at(&[0.3, 0.62, 1.0], |t| [t + 0.25, t, t - 0.25]);
        let bundled = graded(&scan, BUNDLED_CONTRAST, bundled_section(off_desaturation()));
        let split = graded(
            &scan,
            LINEARIZATION,
            LookSection {
                highlight_desaturation: off_desaturation(),
                ..LookSection::default()
            },
        );
        let widest = bundled
            .iter()
            .zip(&split)
            .map(|(a, b)| ((a - b) / a).abs())
            .fold(0.0_f32, f32::max);
        assert!(widest > 1e-3, "no colour difference at all: {widest:e}");
        assert!(
            widest < 0.1,
            "the colour difference is not small: {widest:e}"
        );
    }

    #[test]
    fn highlight_desaturation_sees_the_same_neutral_highlights_either_way() {
        // The band divides by the whole contrast — linearization times the look's — so
        // a roll whose contrast moves from the decode into the look keys the same
        // pixels the same way. Near-white highlights, from half a stop under diffuse
        // white to most of a stop over it, at three density spreads: 0.01 (s ≈ 0.007,
        // below the band: full pull under any nearby divisor) and 0.025 / 0.03
        // (s ≈ 0.018 / 0.021, on the ramp, where the key depends on the divisor).
        use crate::algo::fixed::{BUNDLED_CONTRAST, LINEARIZATION};
        let ts = [0.9, 1.0, 1.1];
        let scan: Vec<f32> = [0.01_f32, 0.025, 0.03]
            .iter()
            .flat_map(|&spread| scan_at(&ts, move |t| [t + spread / 2.0, t, t - spread / 2.0]))
            .collect();
        let bundled = graded(
            &scan,
            BUNDLED_CONTRAST,
            bundled_section(HighlightDesaturation::default()),
        );
        let widest = |split: &[f32]| {
            bundled
                .iter()
                .zip(split)
                .map(|(a, b)| ((a - b) / a).abs())
                .fold(0.0_f32, f32::max)
        };
        let split = graded(&scan, LINEARIZATION, LookSection::default());
        let right = widest(&split);
        // Falsifiability, through the stage: the same split with the band divided by the
        // linearization alone (the look told `LINEARIZATION / DEFAULT_CONTRAST`, so the
        // product it forms is `LINEARIZATION`) keys the ramp pixels differently.
        let wrong = widest(&graded_with(
            &scan,
            LINEARIZATION,
            LINEARIZATION / DEFAULT_CONTRAST,
            LookSection::default(),
        ));
        // Measured 8.6e-5 right and 1.3e-2 wrong.
        assert!(right < 1e-3, "bundled vs split: {right:e}");
        assert!(wrong > 3e-3, "the divisor is not exercised: {wrong:e}");
    }

    #[test]
    fn the_pivot_is_the_mid_grey_the_anchor_pins() {
        // Contrast must pivot at the value the decode's anchor rule places mid-grey at
        // (`MID_GREY_OUTPUT_DECADES` below white), or changing it moves the picture.
        let decades = -MID_GREY.log10();
        let target = crate::types::MID_GREY_OUTPUT_DECADES;
        assert!(
            (decades - target).abs() <= 4.0 * f32::EPSILON * target,
            "{decades} vs {target}"
        );
    }

    #[test]
    fn contrast_pivots_at_mid_grey_and_steepens_symmetrically() {
        for k in [0.5, 1.0, DEFAULT_CONTRAST, 2.0, 3.7] {
            let mut px = [MID_GREY; 3];
            apply_contrast(&mut px, k);
            assert_eq!(px, [MID_GREY; 3], "mid-grey moved at contrast {k}");
            // One stop either side of mid lands k stops either side.
            for stops in [-1.0_f32, 1.0] {
                let mut px = [MID_GREY * stops.exp2(); 3];
                apply_contrast(&mut px, k);
                let got = (px[0] / MID_GREY).log2();
                assert!((got - k * stops).abs() < 1e-5, "k {k}, {stops} stop: {got}");
            }
        }
    }

    #[test]
    fn contrast_one_is_a_bit_exact_identity_and_the_report_says_which_ran() {
        let mut params = LookParams::off();
        assert_eq!(params.applied(), "identity");
        params.section.contrast = 1.3;
        assert!(!params.is_empty());
        assert_eq!(params.applied(), "contrast");
        params.section.highlight_desaturation.strength = 0.5;
        assert_eq!(params.applied(), "contrast+highlight-desaturation");

        // Unity is an identity because the stage *skips* the operator there, not because
        // the operator is one: `0.18 · (x / 0.18)^1` rounds twice and moves some samples
        // by an ULP. So the skip is load-bearing, and both halves are pinned.
        use crate::algo::fixed::{self, DecodeParams};
        use crate::pipeline::working_space::map_nc_film_rgb_v1;
        use crate::types::{FilmBase, LinearImage};
        let scan = scan_at(&DENSITIES, |t| [t + 0.1, t, t - 0.1]);
        let image = LinearImage::new(DENSITIES.len() as u32, 1, scan.clone(), None).unwrap();
        let (film, _) =
            fixed::decode(&image, &FilmBase::from(BASE), &DecodeParams::default()).unwrap();
        let unlooked = map_nc_film_rgb_v1(film).rgb().to_vec();
        let identity = graded(
            &scan,
            fixed::LINEARIZATION,
            bundled_section(off_desaturation()),
        );
        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&identity), bits(&unlooked));
        let mut computed = unlooked.clone();
        for px in computed.chunks_mut(3) {
            let mut p = [px[0], px[1], px[2]];
            apply_contrast(&mut p, 1.0);
            px.copy_from_slice(&p);
        }
        assert_ne!(
            bits(&computed),
            bits(&unlooked),
            "the operator at 1 is exact on this vector, so the skip is untested"
        );
    }

    #[test]
    fn contrast_passes_non_positive_and_non_finite_samples_through() {
        for px in [
            [0.0, 0.3, 0.3],
            [-0.01, 0.3, 0.3],
            [f32::NAN, 0.3, 0.3],
            [f32::INFINITY, 0.3, 0.3],
        ] {
            let mut out = px;
            apply_contrast(&mut out, 1.5);
            assert_eq!(out[0].to_bits(), px[0].to_bits(), "{px:?}");
            assert_ne!(out[1], px[1], "{px:?}: the positive channels still move");
        }
    }

    #[test]
    fn an_unusable_contrast_is_refused() {
        for k in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let section = LookSection {
                contrast: k,
                ..LookSection::default()
            };
            assert!(
                matches!(section.check_contrast(), Err(ContrastFault(v)) if v.to_bits() == k.to_bits()),
                "{k}"
            );
        }
        assert!(LookSection::default().check_contrast().is_ok());
    }

    #[test]
    fn unusable_values_are_refused() {
        let bad = [
            HighlightDesaturation {
                strength: 1.5,
                ..Default::default()
            },
            HighlightDesaturation {
                strength: f32::NAN,
                ..Default::default()
            },
            HighlightDesaturation {
                start_stops: 0.0,
                ..Default::default()
            },
            HighlightDesaturation {
                start_stops: -9.0,
                ..Default::default()
            },
            HighlightDesaturation {
                band: [0.03, 0.02],
                ..Default::default()
            },
            HighlightDesaturation {
                band: [-0.01, 0.02],
                ..Default::default()
            },
        ];
        for p in bad {
            assert!(p.check().is_err(), "{p:?}");
            assert!(Pull::new(&p, 2.0).is_err(), "{p:?}");
        }
        assert!(HighlightDesaturation::default().check().is_ok());
        assert!(Pull::new(&HighlightDesaturation::default(), 0.0).is_err());
    }
}
