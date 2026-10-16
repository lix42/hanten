//! **The roll's white balance as it applies**: the roll's measured gains
//! (`pipeline::roll_white::roll_gains`) either as one gain at every tone (the default), or
//! as a **tilt**, the identity at the film base and the full gains at the roll's white.
//!
//! A gain tints the film base by the roll's white cast; the tilt keeps the base neutral,
//! at the cost of the gains' slight darkening of black. The tilt is held at the gains above
//! the white and at the identity below the base. It is a grade, not a per-roll decode
//! scale, so the decode keeps one global scale (`algo::fixed::DENSITY_SCALE`). Evidence:
//! `docs/progress/nf-scene-correction.md`, `## two-point-tilt`.

use serde::{Deserialize, Serialize};

use crate::pipeline::colorimetry::dot;
use crate::pipeline::colorimetry::pinned::ACESCG_LUMINANCE;
use crate::pipeline::look::MID_GREY;
use crate::pipeline::roll_white::scene_stops;

/// `roll.white_balance_form` / `--white-balance-form`: how the roll's gains apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum WhiteBalanceForm {
    /// One gain at every tone.
    #[default]
    Gain,
    /// A tilt per channel, from the identity at the film base to the gains at the white.
    Tilt,
}

/// The roll's gains as a tilt: per channel, `v · g^t`, where `t` runs in log of the
/// channel's own level from 0 at the decoded film base to 1 at the roll's white, clamped
/// to `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tilt {
    /// The roll's gains, green-anchored.
    pub gains: [f32; 3],
    /// The decoded film base in linear ACEScg.
    pub base: [f32; 3],
    /// Green's level at the roll's white, in scene stops (`roll.white_balance_stops`).
    pub white_stops: f32,
    /// Per channel, `log2` of the base and `1 / log2(white / base)`: `t`'s affine map.
    log_base: [f32; 3],
    inv_span: [f32; 3],
    /// The base's luminance in scene stops, the midtone line's floor under a tilt.
    base_stops: f32,
}

impl Tilt {
    pub fn new(gains: [f32; 3], base: [f32; 3], white_stops: f32) -> Self {
        let green = MID_GREY * white_stops.exp2();
        Self {
            gains,
            base,
            white_stops,
            log_base: base.map(f32::log2),
            inv_span: std::array::from_fn(|c| 1.0 / (green / gains[c] / base[c]).log2()),
            base_stops: scene_stops(dot(base, ACESCG_LUMINANCE)),
        }
    }

    /// The decoded base's luminance in scene stops.
    pub fn base_stops(&self) -> f32 {
        self.base_stops
    }

    /// The one value rule: finite, positive gains and base, and the white above the base
    /// on every channel, so `t` has a span to run over.
    pub fn check(&self) -> Result<(), String> {
        let ok = |v: f32| v.is_finite() && v > 0.0;
        if !(self.gains.into_iter().all(ok) && self.base.into_iter().all(ok)) {
            return Err(format!(
                "the gains {:?} and the decoded base {:?} must be finite and positive",
                self.gains, self.base
            ));
        }
        if !self.inv_span.into_iter().all(ok) {
            return Err(format!(
                "the roll's white ({} scene stops) must sit above the decoded film base on \
                 every channel",
                self.white_stops
            ));
        }
        Ok(())
    }

    /// The tilted pixel. A channel at or below zero, or not finite, has no log and is left
    /// alone.
    pub fn apply(&self, px: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|c| {
            let t = ((px[c].log2() - self.log_base[c]) * self.inv_span[c]).clamp(0.0, 1.0);
            if t.is_nan() {
                px[c]
            } else {
                px[c] * self.gains[c].powf(t)
            }
        })
    }
}

/// The roll's white balance as the midtone line and `measure-roll` read it. Scene
/// correction applies a [`RollBalance::Gain`] folded into its stated gains (one multiply),
/// a [`RollBalance::Tilt`] on its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RollBalance {
    Gain([f32; 3]),
    Tilt(Tilt),
}

impl RollBalance {
    /// The balanced pixel.
    pub fn apply(&self, px: [f32; 3]) -> [f32; 3] {
        match self {
            RollBalance::Gain(g) => [px[0] * g[0], px[1] * g[1], px[2] * g[2]],
            RollBalance::Tilt(t) => t.apply(px),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tilt() -> Tilt {
        Tilt::new([0.85, 1.0, 1.06], [0.01; 3], 2.0)
    }

    /// Each channel's level at the roll's white, before the gains.
    fn white(t: &Tilt) -> [f32; 3] {
        let green = MID_GREY * t.white_stops.exp2();
        t.gains.map(|g| green / g)
    }

    fn close(got: [f32; 3], want: [f32; 3]) {
        for c in 0..3 {
            assert!(
                (got[c] / want[c] - 1.0).abs() < 1e-5,
                "{c}: {got:?} vs {want:?}"
            );
        }
    }

    #[test]
    fn a_tilt_is_the_identity_at_the_base_and_the_gains_at_the_white() {
        let t = tilt();
        t.check().unwrap();
        close(t.apply(t.base), t.base);
        let w = white(&t);
        close(t.apply(w), std::array::from_fn(|c| w[c] * t.gains[c]));
    }

    #[test]
    fn between_base_and_white_it_is_a_power_pivoted_at_the_base() {
        let t = tilt();
        let w = white(&t);
        // Halfway in log between base and white: half the gain, in log.
        let mid: [f32; 3] = std::array::from_fn(|c| (t.base[c] * w[c]).sqrt());
        close(
            t.apply(mid),
            std::array::from_fn(|c| mid[c] * t.gains[c].sqrt()),
        );
    }

    #[test]
    fn it_holds_the_gains_above_the_white_and_the_identity_below_the_base() {
        let t = tilt();
        let above = white(&t).map(|v| v * 4.0);
        close(
            t.apply(above),
            std::array::from_fn(|c| above[c] * t.gains[c]),
        );
        let below = t.base.map(|v| v / 4.0);
        assert_eq!(t.apply(below), below);
    }

    #[test]
    fn a_level_with_no_log_is_left_alone() {
        let t = tilt();
        let out = t.apply([0.0, -0.1, f32::NAN]);
        assert_eq!(out[..2], [0.0, -0.1]);
        assert!(out[2].is_nan());
    }

    #[test]
    fn a_white_at_or_under_the_base_is_refused() {
        let t = Tilt::new([0.85, 1.0, 1.06], [0.01; 3], (0.005f32 / MID_GREY).log2());
        assert!(t.check().is_err());
        assert!(
            Tilt::new([0.85, 1.0, 1.06], [0.01; 3], f32::NAN)
                .check()
                .is_err()
        );
    }

    #[test]
    fn the_gain_form_multiplies_as_scene_correction_always_has() {
        let g = [1.21, 1.0, 1.15];
        let px = [0.3, 0.2, 0.1];
        assert_eq!(
            RollBalance::Gain(g).apply(px),
            [px[0] * g[0], px[1] * g[1], px[2] * g[2]]
        );
    }
}
