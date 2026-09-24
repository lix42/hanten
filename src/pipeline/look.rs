//! **Stage 2 of the new rendering chain — the look.**
//!
//! The creative stage: highlight desaturation today, and later contrast, the
//! per-channel grade with a mid-grey pivot, print emulation and per-stock
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
use crate::pipeline::colorimetry::pinned::ACESCG_LUMA;
use crate::pipeline::fit_range::dot;
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
///      s = log10(max/min) / contrast over the pixel's ACEScg channels
/// rgb ← rgb + strength · b · w · (Y − rgb)            Y = ACEScg luminance
/// ```
///
/// - **Keyed on distance from the neutral axis as well as brightness**, or a bright
///   *coloured* surface is neutralised as hard as a bright white one — the sunset case
///   `docs/spike/highlight-desaturation.md` found on one marked patch. `s` is the
///   negative's own density spread (the log ratio over the decode's contrast), so the
///   band means the same on a flat roll and a contrasty one.
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

/// The recipe's `look` section — one key per control (`nf-look/stage`) — and what the
/// report echoes as resolved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LookSection {
    pub highlight_desaturation: HighlightDesaturation,
}

impl LookSection {
    /// Whether the look moves no pixel — what the report's `applied` reads.
    pub fn is_empty(&self) -> bool {
        self.highlight_desaturation.is_off()
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

/// The look's parameters: the recipe's `look` section plus the decode's contrast,
/// which the saturation measure is normalised by. **No `Default`**, for the reason
/// `FitRangeParams` has none: the contrast is the decode's to state, and
/// `Recipe::chain_params` adds it.
#[derive(Clone, Debug, PartialEq)]
pub struct LookParams {
    pub section: LookSection,
    /// The decode's contrast (`reconstruction.contrast`).
    pub decode_contrast: f32,
}

impl LookParams {
    /// **Test fixture**: an empty look (strength 0) at the fixed decode's default
    /// contrast — for tests that are not about the look.
    #[cfg(test)]
    pub fn off() -> Self {
        let mut section = LookSection::default();
        section.highlight_desaturation.strength = 0.0;
        Self {
            section,
            decode_contrast: crate::algo::fixed::CONTRAST,
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
        if self.is_empty() {
            "identity"
        } else {
            "highlight-desaturation"
        }
    }
}

/// Linear ACEScg after the look: scene-referred, graded.
///
/// This is also **the one source both display branches share**. The branch point
/// is below it (`nf-display-stages/branch-contract`), so whatever that task
/// decides about the split, this is the boundary it splits *from*.
pub struct GradedImage(WorkingBuffer);

impl GradedImage {
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

/// Apply the look. An empty look returns the buffer untouched, bit for bit.
///
/// Nothing is clamped, and a non-finite sample passes through untouched for fit range
/// to refuse by name.
pub fn apply(image: SceneReferredImage, params: &LookParams) -> Result<GradedImage> {
    let mut buffer = image.into_buffer();
    if !params.is_empty() {
        let pull = Pull::new(
            &params.section.highlight_desaturation,
            params.decode_contrast,
        )?;
        pixels::map_in_place(buffer.rgb_mut(), |px| pull.apply(px));
    }
    Ok(GradedImage(buffer))
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
    contrast: f32,
}

impl Pull {
    fn new(p: &HighlightDesaturation, contrast: f32) -> Result<Self> {
        if p.check().is_err() || !(contrast.is_finite() && contrast > 0.0) {
            return Err(NcError::Other(format!(
                "highlight desaturation was handed unusable parameters ({p:?} at decode \
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
        let with = |f: fn(&mut HighlightDesaturation)| {
            let mut section = LookSection::default();
            f(&mut section.highlight_desaturation);
            section
        };
        // The default does something, yet asks for nothing.
        let default = LookSection::default();
        assert!(!default.is_empty() && !default.asks_for_a_look());
        // Off is an identity however its inert knobs sit.
        assert!(!with(|h| h.strength = 0.0).asks_for_a_look());
        assert!(
            !with(|h| {
                h.strength = 0.0;
                h.band = [0.02, 0.04];
                h.start_stops = -2.0;
            })
            .asks_for_a_look()
        );
        assert!(with(|h| h.strength = 0.5).asks_for_a_look());
        assert!(with(|h| h.band = [0.02, 0.04]).asks_for_a_look());
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
