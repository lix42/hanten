//! **Stage 1 of the new rendering chain — scene correction.**
//!
//! Photographic corrections toward what the scene was: **white balance** and
//! **exposure**, and later the flare/fog half of the black point
//! (`nf-scene-correction/flare-removal`). Scene-referred and linear: each is a
//! per-channel gain on linear ACEScg, so the two fold into one multiply and nothing
//! is clamped.
//!
//! Written fresh, per CLAUDE.md's migration rule. `render_split::apply_shared_controls`
//! runs the current chain's version of the same arithmetic, fused with the black
//! point and `linear_range`; it is context for what the controls do, not a template.
//!
//! The corrections act on working-space channels, **after** the NC film RGB v1 3×3 —
//! so this white balance is not reconstruction's `offset`, and this exposure is not
//! its anchor (`docs/design-update.md` Part 1 measures the two white-balance bases
//! ≈2.6 % apart on a neutral). With the anchor a reference-free convention,
//! exposure here is where brightness is set.
//!
//! **Where an already-positive scan will enter** (`io/positive-input-mode`): ahead of
//! this stage, at the working space — a positive is brought to linear ACEScg and
//! then corrected like a negative, because a slide needs white balance and exposure
//! as much as a negative does. So [`AcesCgImage`] is the chain's entry for both, and
//! [`apply`] stays the only producer of a [`SceneReferredImage`].

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::pipeline::pixels;
use crate::pipeline::white_balance::{self, Estimator};
use crate::pipeline::working_image::WorkingBuffer;
use crate::pipeline::working_space::AcesCgImage;
use crate::types::{NcError, Result};

/// Where the white-balance gains come from: one mutually-exclusive choice, so
/// precedence is by **source** — an explicit `--white-balance 1,1,1` over a recipe's
/// auto mode means neutral gains, not a re-estimate.
///
/// Serialized as `{ "explicit": [r, g, b] }`, `"gray-world"` or `"percentile"`.
/// Unlike the current chain's `print.white_balance` it accepts no bare `[r, g, b]`
/// array: that form is a compatibility alias for recipes older than the tagged one,
/// and this recipe has none.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WhiteBalance {
    /// Stated per-channel gains. The default `[1, 1, 1]` is neutral.
    Explicit([f32; 3]),
    /// Estimated per frame over the measurement region: equalize the trimmed
    /// channel means.
    GrayWorld,
    /// Estimated per frame over the measurement region: equalize the channels at a
    /// near-white percentile.
    Percentile,
}

impl Default for WhiteBalance {
    fn default() -> Self {
        WhiteBalance::Explicit([1.0, 1.0, 1.0])
    }
}

impl WhiteBalance {
    /// The estimator an auto mode runs, or `None` for stated gains.
    pub fn estimator(self) -> Option<Estimator> {
        match self {
            WhiteBalance::Explicit(_) => None,
            WhiteBalance::GrayWorld => Some(Estimator::GrayWorld),
            WhiteBalance::Percentile => Some(Estimator::Percentile),
        }
    }
}

/// Scene correction's knobs — and the new chain's `scene_correction` recipe section
/// (`crate::recipe`), field for field.
///
/// A struct rather than an `Option`: a stage is always in the chain, and "this stage
/// is off" is deliberately not expressible — the defaults are the identity, and the
/// report says so.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SceneCorrectionParams {
    /// Where the white-balance gains come from (default neutral).
    pub white_balance: WhiteBalance,
    /// Exposure in stops (EV); `0` is neutral. Applied as the gain `2^exposure`.
    pub exposure: f32,
}

impl Default for SceneCorrectionParams {
    fn default() -> Self {
        Self {
            white_balance: WhiteBalance::default(),
            exposure: 0.0,
        }
    }
}

/// A value [`SceneCorrectionParams::check`] refuses, carried as data so each caller
/// can name the knob the way its command spells it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SceneFault {
    /// A stated white-balance gain is not finite and positive.
    WhiteBalance { channel: usize, value: f32 },
    /// The exposure is not finite, or its gain `2^exposure` is not a normal `f32`.
    Exposure(f32),
    /// A gain times the exposure gain is not a positive normal `f32` — a channel
    /// would render as `0`, `inf` or inverted. Reached from `check` only when each is
    /// usable alone; from `apply`, also by a stated gain `check` never saw.
    Combined { channel: usize, gain: f32 },
}

impl SceneCorrectionParams {
    /// The value rules on these parameters as stated. Auto-estimated gains are
    /// checked again when [`apply`] resolves them, where the same rule is a runtime
    /// fault rather than a usage one.
    pub fn check(&self) -> std::result::Result<(), SceneFault> {
        if let WhiteBalance::Explicit(gains) = self.white_balance {
            for (channel, &value) in gains.iter().enumerate() {
                if !value.is_finite() || value <= 0.0 {
                    return Err(SceneFault::WhiteBalance { channel, value });
                }
            }
        }
        let exposure_gain = exposure_gain(self.exposure)?;
        if let WhiteBalance::Explicit(gains) = self.white_balance {
            combined_gains(gains, exposure_gain)?;
        }
        Ok(())
    }

    /// Whether this stage measures anything over the frame's measurement region —
    /// which makes an empty region fatal for the run rather than a warning.
    pub fn measures_over_region(&self) -> bool {
        self.white_balance.estimator().is_some()
    }
}

/// `2^stops`, refused unless it is a **normal** `f32`: an exposure far enough from
/// zero underflows to a subnormal or `0` (every sample silently black) or overflows
/// to `inf`.
fn exposure_gain(stops: f32) -> std::result::Result<f32, SceneFault> {
    let gain = stops.exp2();
    if !stops.is_finite() || !gain.is_normal() {
        return Err(SceneFault::Exposure(stops));
    }
    Ok(gain)
}

/// Fold the white-balance gains and the exposure gain into the one per-channel
/// multiplier the stage applies, refusing a product that is not a normal `f32`.
fn combined_gains(
    white_balance: [f32; 3],
    exposure_gain: f32,
) -> std::result::Result<[f32; 3], SceneFault> {
    let mut gains = [0.0; 3];
    for (channel, (out, wb)) in gains.iter_mut().zip(white_balance).enumerate() {
        let gain = wb * exposure_gain;
        // Positive too: a negative gain is normal, and would invert the channel.
        if !gain.is_normal() || gain < 0.0 {
            return Err(SceneFault::Combined { channel, gain });
        }
        *out = gain;
    }
    Ok(gains)
}

/// Where the applied white-balance gains came from.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "provenance", rename_all = "kebab-case")]
pub enum WhiteBalanceSource {
    /// Stated by the recipe or `--white-balance`.
    Stated,
    /// Estimated from this frame, over the measurement region `[x, y, w, h]`.
    Estimated {
        estimator: &'static str,
        region: [u32; 4],
    },
}

/// What scene correction applied to one frame: the values **as resolved**, which is
/// what the report states and what a user copies into a recipe to reproduce an
/// estimated frame exactly (`--white-balance` with these gains).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SceneCorrection {
    /// The white-balance gains applied, green-anchored when estimated.
    pub white_balance: [f32; 3],
    #[serde(flatten)]
    pub source: WhiteBalanceSource,
    /// The exposure applied, in stops.
    pub exposure: f32,
}

impl SceneCorrection {
    /// What the stage did, for the report's stage list. Derived from the resolved
    /// **gains**, by the same test [`apply`] uses to decide whether to touch a pixel,
    /// so the report never names an operation that moved none: an estimate landing
    /// exactly on neutral, an exposure too small to change `2^EV` from `1.0`, and a
    /// white balance the exposure cancels all read as `"identity"`.
    pub fn applied(&self) -> &'static str {
        let exposure_gain = self.exposure.exp2();
        if self.white_balance.map(|wb| wb * exposure_gain) == [1.0, 1.0, 1.0] {
            return "identity";
        }
        let white_balance = self.white_balance != [1.0, 1.0, 1.0];
        let exposure = exposure_gain != 1.0;
        match (white_balance, exposure) {
            (false, false) => "identity",
            (true, false) => "white-balance",
            (false, true) => "exposure",
            (true, true) => "white-balance+exposure",
        }
    }
}

/// Linear ACEScg after scene correction: still **scene-referred**, now carrying
/// the photographic corrections.
///
/// The field is private to this module, so [`apply`] is the only function that
/// can mint one — the [`AcesCgImage`] pattern, and what keeps a buffer that
/// skipped the stage out of the next one.
pub struct SceneReferredImage(WorkingBuffer);

impl SceneReferredImage {
    /// Hand the buffer to the next stage. Consuming, so the pixels move rather
    /// than copy.
    pub(in crate::pipeline) fn into_buffer(self) -> WorkingBuffer {
        self.0
    }
}

impl fmt::Debug for SceneReferredImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_named(f, "SceneReferredImage")
    }
}

/// Resolve and apply scene correction: `v_c ← v_c · wb_c · 2^exposure` per channel.
///
/// An auto white balance is estimated over `measure_region` (`[x, y, w, h]`, the
/// frame's effective area), sampled from this stage's own input — so the estimate
/// sees the same pixels the gains then multiply. `measure_region` is read only by
/// an auto mode, and an auto mode without one is refused: the orchestrator refuses
/// an empty region before the render, so reaching here without one is a wiring bug.
///
/// The identity configuration returns the buffer untouched, bit for bit. Otherwise
/// nothing is clamped (clamping happens only at the encoder) and a non-finite sample
/// stays non-finite, for the encoder to count.
pub fn apply(
    image: AcesCgImage,
    params: &SceneCorrectionParams,
    measure_region: Option<[u32; 4]>,
) -> Result<(SceneReferredImage, SceneCorrection)> {
    let (white_balance, source) = match params.white_balance {
        WhiteBalance::Explicit(gains) => (gains, WhiteBalanceSource::Stated),
        auto => {
            let estimator = auto.estimator().expect("an auto mode has an estimator");
            let region = measure_region.ok_or_else(|| {
                NcError::Other(format!(
                    "auto white balance ({}) has no measurement region to estimate over",
                    estimator.name()
                ))
            })?;
            let sampled = white_balance::sample_region(image.rgb(), image.width(), region)?;
            let gains = white_balance::estimate_gains(&sampled, estimator)?;
            let source = WhiteBalanceSource::Estimated {
                estimator: estimator.name(),
                region,
            };
            (gains, source)
        }
    };
    // `SceneCorrectionParams::check` has already refused every stated value this can
    // fault on, so what reaches here is an *estimated* gain meeting the exposure.
    let gains = exposure_gain(params.exposure)
        .and_then(|e| combined_gains(white_balance, e))
        .map_err(|fault| {
            let why = match fault {
                SceneFault::Combined { channel, gain } => format!(
                    "white-balance gain {} on channel {channel} times the exposure \
                     gain is {gain:e}, which is not a positive normal f32 — every \
                     sample of that channel would render as 0, inf or inverted",
                    white_balance[channel]
                ),
                SceneFault::Exposure(_) | SceneFault::WhiteBalance { .. } => {
                    "the exposure gain 2^EV is not a normal f32".into()
                }
            };
            NcError::Other(format!(
                "scene correction cannot apply exposure {} EV to white balance \
                 {white_balance:?}: {why}. Move the exposure (`--exposure` / \
                 `scene_correction.exposure`) toward 0",
                params.exposure
            ))
        })?;

    let mut buffer = WorkingBuffer::from_aces(image);
    if gains != [1.0, 1.0, 1.0] {
        pixels::map_in_place(buffer.rgb_mut(), |px| {
            for c in 0..3 {
                px[c] *= gains[c];
            }
        });
    }
    let resolved = SceneCorrection {
        white_balance,
        source,
        exposure: params.exposure,
    };
    Ok((SceneReferredImage(buffer), resolved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::FilmRgbImage;
    use crate::pipeline::working_space::map_nc_film_rgb_v1;
    use crate::types::LinearImage;

    /// An `AcesCgImage` whose *film RGB* was exactly `rgb`.
    fn aces_from(width: u32, height: u32, rgb: &[f32]) -> AcesCgImage {
        let film =
            FilmRgbImage::fixture(LinearImage::new(width, height, rgb.to_vec(), None).unwrap());
        map_nc_film_rgb_v1(film)
    }

    fn bits(pixels: &[f32]) -> Vec<u32> {
        pixels.iter().map(|v| v.to_bits()).collect()
    }

    fn run(
        image: AcesCgImage,
        params: &SceneCorrectionParams,
        region: Option<[u32; 4]>,
    ) -> Result<(Vec<f32>, SceneCorrection)> {
        let (out, resolved) = apply(image, params, region)?;
        Ok((out.into_buffer().into_linear().rgb, resolved))
    }

    #[test]
    fn the_default_is_a_bit_exact_identity_and_says_so() {
        let rgb = [
            0.0,
            0.18,
            1.0,
            5.0,
            -0.25,
            f32::INFINITY,
            f32::NAN,
            -0.0,
            0.5,
        ];
        let aces = aces_from(3, 1, &rgb);
        let before = bits(aces.rgb());
        let (out, resolved) = run(aces, &SceneCorrectionParams::default(), None).unwrap();
        assert_eq!(bits(&out), before);
        assert_eq!(resolved.applied(), "identity");
        assert_eq!(resolved.source, WhiteBalanceSource::Stated);
    }

    #[test]
    fn stated_white_balance_and_exposure_reproduce_a_hand_computed_value() {
        let aces = aces_from(2, 1, &[0.2, 0.4, 0.6, 1.5, f32::NAN, -0.1]);
        let input = aces.rgb().to_vec();
        let params = SceneCorrectionParams {
            white_balance: WhiteBalance::Explicit([1.25, 1.0, 0.5]),
            exposure: 1.0,
        };
        let (out, resolved) = run(aces, &params, None).unwrap();
        // One multiply per sample by the folded gain `wb_c · 2^1`, exactly.
        let gains = [2.5f32, 2.0, 1.0];
        for (i, (&o, &v)) in out.iter().zip(&input).enumerate() {
            let want = v * gains[i % 3];
            assert_eq!(o.to_bits(), want.to_bits(), "sample {i}: {o} vs {want}");
        }
        assert!(out[4].is_nan(), "a non-finite sample stays non-finite");
        assert_eq!(resolved.applied(), "white-balance+exposure");
        assert_eq!(resolved.white_balance, [1.25, 1.0, 0.5]);
        assert_eq!(resolved.exposure, 1.0);
    }

    #[test]
    fn the_report_names_each_correction_that_ran() {
        let cases = [
            (WhiteBalance::Explicit([1.0, 1.0, 1.0]), 0.0, "identity"),
            (
                WhiteBalance::Explicit([1.1, 1.0, 0.9]),
                0.0,
                "white-balance",
            ),
            (WhiteBalance::Explicit([1.0, 1.0, 1.0]), -0.5, "exposure"),
            // `2^1e-9` rounds to exactly 1.0: no pixel moves, so nothing is claimed.
            (WhiteBalance::Explicit([1.0, 1.0, 1.0]), 1e-9, "identity"),
            // A white balance the exposure cancels exactly is also a no-op.
            (WhiteBalance::Explicit([2.0, 2.0, 2.0]), -1.0, "identity"),
        ];
        for (white_balance, exposure, want) in cases {
            let params = SceneCorrectionParams {
                white_balance,
                exposure,
            };
            let aces = aces_from(1, 1, &[0.3, 0.3, 0.3]);
            let before = bits(aces.rgb());
            let (out, resolved) = run(aces, &params, None).unwrap();
            assert_eq!(resolved.applied(), want, "{params:?}");
            // The label and the pixels agree: "identity" exactly when nothing moved.
            assert_eq!(want == "identity", bits(&out) == before, "{params:?}");
        }
    }

    #[test]
    fn auto_white_balance_estimates_over_the_region_and_reports_its_provenance() {
        // Left column neutral grey, right column a strong cast. A region covering only
        // the left column must estimate neutral; the whole frame must not — which is
        // what proves the region, and not the frame, was sampled.
        let (w, h) = (2u32, 2u32);
        let rgb = [
            0.3, 0.3, 0.3, 0.9, 0.3, 0.1, //
            0.3, 0.3, 0.3, 0.9, 0.3, 0.1,
        ];
        for mode in [WhiteBalance::GrayWorld, WhiteBalance::Percentile] {
            let params = SceneCorrectionParams {
                white_balance: mode,
                exposure: 0.0,
            };
            let (_, left) = run(aces_from(w, h, &rgb), &params, Some([0, 0, 1, 2])).unwrap();
            let (_, whole) = run(aces_from(w, h, &rgb), &params, Some([0, 0, 2, 2])).unwrap();
            for c in 0..3 {
                assert!(
                    (left.white_balance[c] - 1.0).abs() < 1e-5,
                    "{mode:?}: a neutral region estimates neutral, got {:?}",
                    left.white_balance
                );
            }
            assert_ne!(whole.white_balance, left.white_balance, "{mode:?}");
            assert_eq!(
                left.source,
                WhiteBalanceSource::Estimated {
                    estimator: mode.estimator().unwrap().name(),
                    region: [0, 0, 1, 2],
                }
            );
        }
    }

    #[test]
    fn estimated_gains_are_the_ones_applied() {
        // Reusing the reported gains as stated ones reproduces the render bit for
        // bit — the "measure once, reuse" contract.
        let rgb = [0.2, 0.3, 0.5, 0.4, 0.5, 0.7, 0.1, 0.2, 0.2, 0.6, 0.6, 0.9];
        let auto = SceneCorrectionParams {
            white_balance: WhiteBalance::GrayWorld,
            exposure: 0.5,
        };
        let (estimated, resolved) = run(aces_from(2, 2, &rgb), &auto, Some([0, 0, 2, 2])).unwrap();
        let stated = SceneCorrectionParams {
            white_balance: WhiteBalance::Explicit(resolved.white_balance),
            exposure: 0.5,
        };
        let (reused, _) = run(aces_from(2, 2, &rgb), &stated, None).unwrap();
        assert_eq!(bits(&estimated), bits(&reused));
    }

    #[test]
    fn auto_white_balance_without_a_region_is_refused() {
        let params = SceneCorrectionParams {
            white_balance: WhiteBalance::Percentile,
            exposure: 0.0,
        };
        assert!(run(aces_from(1, 1, &[0.3, 0.3, 0.3]), &params, None).is_err());
    }

    #[test]
    fn unusable_values_are_refused_before_any_pixel_moves() {
        let bad = [
            (WhiteBalance::Explicit([1.0, 0.0, 1.0]), 0.0),
            (WhiteBalance::Explicit([1.0, -1.0, 1.0]), 0.0),
            (WhiteBalance::Explicit([f32::NAN, 1.0, 1.0]), 0.0),
            (WhiteBalance::default(), f32::INFINITY),
            (WhiteBalance::default(), 200.0),
            (WhiteBalance::default(), -200.0),
            // Each usable alone, but the product underflows to a subnormal.
            (WhiteBalance::Explicit([1e-30, 1.0, 1.0]), -100.0),
        ];
        for (white_balance, exposure) in bad {
            let params = SceneCorrectionParams {
                white_balance,
                exposure,
            };
            assert!(params.check().is_err(), "check must refuse {params:?}");
            assert!(
                run(aces_from(1, 1, &[0.3, 0.3, 0.3]), &params, None).is_err(),
                "apply must refuse {params:?}"
            );
        }
        assert_eq!(SceneCorrectionParams::default().check(), Ok(()));
    }

    #[test]
    fn a_reported_estimator_name_loads_back_as_that_mode() {
        // The report's `estimator` and the recipe's `white_balance` spell the same
        // modes in two places; a user copying one into the other must get the mode
        // back, so every auto mode's report name must deserialize to itself.
        for mode in [WhiteBalance::GrayWorld, WhiteBalance::Percentile] {
            let name = mode.estimator().unwrap().name();
            let back: WhiteBalance = serde_json::from_value(serde_json::json!(name)).unwrap();
            assert_eq!(back, mode, "{name}");
        }
    }

    #[test]
    fn the_recipe_section_round_trips_and_refuses_a_bare_array() {
        let params = SceneCorrectionParams {
            white_balance: WhiteBalance::Explicit([1.1, 1.0, 0.9]),
            exposure: 0.25,
        };
        let json = serde_json::to_string(&params).unwrap();
        assert_eq!(
            json,
            r#"{"white_balance":{"explicit":[1.1,1.0,0.9]},"exposure":0.25}"#
        );
        assert_eq!(
            serde_json::from_str::<SceneCorrectionParams>(&json).unwrap(),
            params
        );
        let auto: SceneCorrectionParams =
            serde_json::from_str(r#"{"white_balance":"gray-world"}"#).unwrap();
        assert_eq!(auto.white_balance, WhiteBalance::GrayWorld);
        assert!(
            serde_json::from_str::<SceneCorrectionParams>(r#"{"white_balance":[1,1,1]}"#).is_err()
        );
    }
}
