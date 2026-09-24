//! Auto white balance: deterministic per-channel statistics that estimate
//! green-anchored gains from a sample of a rendered positive.
//!
//! Owned by the new chain's scene correction (`pipeline::scene_correction`), which
//! samples its measurement region. The current chain's white-balance site, the
//! shared display controls (`render_split`), reaches the same estimator through
//! [`resolve_print_gains`], which keeps its whole-frame sample, so its output is
//! unchanged by the move.
//!
//! Pure statistics, no ML (the project's "AI-friendly ≠ ML" rule): same sample and
//! estimator ⇒ identical gains.

use crate::types::{NcError, Result, WbSource};

/// Which statistic an auto white balance equalizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Estimator {
    /// Equalize the trimmed per-channel means (≈ NLP Auto-AVG). Assumes the
    /// sample averages to neutral, so a dominant scene colour biases it.
    GrayWorld,
    /// Equalize the channels at a matched near-white percentile (≈ NLP
    /// Auto-Neutral). More robust to a dominant colour — highlights are where
    /// neutrality matters most.
    Percentile,
}

impl Estimator {
    /// The estimator's name as recipes and reports spell it.
    pub fn name(self) -> &'static str {
        match self {
            Estimator::GrayWorld => "gray-world",
            Estimator::Percentile => "percentile",
        }
    }
}

/// Cap on how many *pixels* the statistics examine. ~1M pixels are statistically
/// indistinguishable from the full population for a mean/percentile, and the cap
/// bounds the analysis to a small transient buffer per channel on large scans.
const AUTO_WB_MAX_PIXELS: usize = 1 << 20;

/// Percentile equalized by [`Estimator::Percentile`] (per channel, nearest rank).
/// High enough to sit on near-white content, while the top 5% (specular sparkle,
/// dust, would-be-clipped extremes) never enters the statistic.
const AUTO_WB_PERCENTILE: f32 = 0.95;

/// Fraction trimmed from *each* end of a channel's distribution before the
/// [`Estimator::GrayWorld`] mean, so dead blacks and clipped/specular extremes
/// can't skew it. Frame-relative (a quantile, not an absolute level), so it works
/// for display-anchored and scene-referred renders alike.
const AUTO_WB_TRIM: f32 = 0.01;

/// Deterministic pixel stride: the smallest that keeps the examined pixel count
/// under [`AUTO_WB_MAX_PIXELS`]. Strides whole pixels, so every sampled pixel
/// contributes all three channels.
fn auto_wb_stride(pixels: usize) -> usize {
    pixels.div_ceil(AUTO_WB_MAX_PIXELS).max(1)
}

/// Sample a whole interleaved-RGB frame on the deterministic [`auto_wb_stride`].
///
/// The current chain's sample. [`estimate_gains`] must not stride again: it
/// examines exactly the set it is given.
pub(crate) fn sample_frame(rgb: &[f32]) -> Vec<f32> {
    let pixels = rgb.len() / 3;
    let stride = auto_wb_stride(pixels);
    let mut sampled = Vec::with_capacity(pixels.div_ceil(stride) * 3);
    for px in rgb.as_chunks::<3>().0.iter().step_by(stride) {
        sampled.extend_from_slice(px);
    }
    sampled
}

/// Sample the rectangle `[x, y, w, h]` of a `width`-wide interleaved-RGB frame on
/// the same deterministic stride, taken over the region's own row-major pixel
/// index — so the sample depends on the region, never on the frame around it.
///
/// Fails on a region that is empty or leaves the frame: a caller that reaches
/// here with one has resolved the wrong rectangle, and sampling what remains
/// would estimate over pixels nobody chose.
pub(crate) fn sample_region(rgb: &[f32], width: u32, region: [u32; 4]) -> Result<Vec<f32>> {
    let [x, y, w, h] = region.map(|v| v as usize);
    let width = width as usize;
    let height = (rgb.len() / 3).checked_div(width).unwrap_or(0);
    if w == 0 || h == 0 || x + w > width || y + h > height {
        return Err(NcError::Other(format!(
            "auto white balance: the measurement region {region:?} is empty or leaves \
             the {width}x{height} frame"
        )));
    }
    let pixels = w * h;
    let stride = auto_wb_stride(pixels);
    let mut sampled = Vec::with_capacity(pixels.div_ceil(stride) * 3);
    for i in (0..pixels).step_by(stride) {
        let at = ((y + i / w) * width + x + i % w) * 3;
        sampled.extend_from_slice(&rgb[at..at + 3]);
    }
    Ok(sampled)
}

/// Per-channel *finite* samples of an already-sampled `rgb`, each channel sorted
/// ascending (`total_cmp`). Non-finite samples are excluded per sample, so a bad
/// pixel can't poison a statistic. The full sort makes every downstream statistic
/// order-defined, hence deterministic.
fn wb_channel_samples(rgb: &[f32]) -> [Vec<f32>; 3] {
    let cap = rgb.len() / 3;
    let mut channels = [
        Vec::with_capacity(cap),
        Vec::with_capacity(cap),
        Vec::with_capacity(cap),
    ];
    for px in rgb.as_chunks::<3>().0 {
        for (c, channel) in channels.iter_mut().enumerate() {
            if px[c].is_finite() {
                channel.push(px[c]);
            }
        }
    }
    for channel in &mut channels {
        channel.sort_unstable_by(f32::total_cmp);
    }
    channels
}

/// Nearest-rank percentile of a sorted, non-empty slice (`round((n−1)·p)`, the
/// same convention as `density::auto_dmax`).
fn nearest_rank(sorted: &[f32], p: f32) -> f32 {
    sorted[(((sorted.len() - 1) as f32) * p).round() as usize]
}

/// Mean of the central `[trim, 1 − trim]` quantile span of a sorted, non-empty
/// slice. Accumulates in `f64` sequentially over the sorted order — a fully
/// order-defined sum, so the result is deterministic (a parallel float reduction
/// would not be).
fn trimmed_mean(sorted: &[f32], trim: f32) -> f32 {
    let lo = (((sorted.len() - 1) as f32) * trim).round() as usize;
    let hi = (((sorted.len() - 1) as f32) * (1.0 - trim)).round() as usize;
    let span = &sorted[lo..=hi];
    (span.iter().map(|&v| f64::from(v)).sum::<f64>() / span.len() as f64) as f32
}

/// The remedy when an estimate fails. Names the recipe key as well as the flag:
/// `roll` takes no conversion flags, and this estimator serves both chains, whose
/// keys differ only in their section (`print.` / `scene_correction.`).
const STATE_GAINS_INSTEAD: &str = "state explicit gains instead (`--white-balance` on \
     `convert`, or the recipe's `white_balance` key as `{\"explicit\": [r, g, b]}`)";

/// Estimate white-balance gains `[r, g, b]` from an **already-sampled** positive.
///
/// Distribution extremes are excluded by construction (the percentile's top tail,
/// the trimmed mean), so clipped speculars and dead pixels don't skew the estimate.
/// Gains are **green-anchored** (`g = 1`): white balance corrects *colour*, not
/// brightness — that is exposure's job. Fails loudly ([`NcError::Other`], exit 1)
/// when a channel yields no usable level (all samples non-finite, or a non-positive
/// level no multiplicative gain can correct) — never silently-neutral or garbage
/// gains.
pub(crate) fn estimate_gains(rgb: &[f32], estimator: Estimator) -> Result<[f32; 3]> {
    let mode = estimator.name();
    let level_of = |sorted: &[f32]| match estimator {
        Estimator::GrayWorld => trimmed_mean(sorted, AUTO_WB_TRIM),
        Estimator::Percentile => nearest_rank(sorted, AUTO_WB_PERCENTILE),
    };

    let channels = wb_channel_samples(rgb);
    let mut level = [0.0f32; 3];
    for (c, name) in ["red", "green", "blue"].into_iter().enumerate() {
        let l = if channels[c].is_empty() {
            f32::NAN // no usable sample in this channel
        } else {
            level_of(&channels[c])
        };
        if !l.is_finite() || l <= 0.0 {
            return Err(NcError::Other(format!(
                "auto white balance ({mode}): the {name} channel has no usable \
                 level (got {l}); {STATE_GAINS_INSTEAD}"
            )));
        }
        level[c] = l;
    }

    let gains = [level[1] / level[0], 1.0, level[1] / level[2]];
    for (g, name) in gains.into_iter().zip(["red", "green", "blue"]) {
        // Positive finite levels can still divide into inf/0 across an extreme
        // dynamic range (subnormal denominators); guard the gains themselves.
        if !g.is_finite() || g <= 0.0 {
            return Err(NcError::Other(format!(
                "auto white balance ({mode}): estimated {name} gain is not a \
                 positive finite value (got {g}); {STATE_GAINS_INSTEAD}"
            )));
        }
    }
    Ok(gains)
}

/// The current chain's white balance: explicit gains pass through; an auto mode is
/// estimated over a whole-frame sample of `rgb`.
///
/// Scaffolding for the current chain's display branch, deleted with `print.white_balance`
/// by the retirement epic.
pub(crate) fn resolve_print_gains(rgb: &[f32], source: WbSource) -> Result<[f32; 3]> {
    let estimator = match source {
        WbSource::Explicit(gains) => return Ok(gains),
        WbSource::GrayWorld => Estimator::GrayWorld,
        WbSource::Percentile => Estimator::Percentile,
    };
    estimate_gains(&sample_frame(rgb), estimator)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    const BOTH: [Estimator; 2] = [Estimator::GrayWorld, Estimator::Percentile];

    /// An interleaved RGB buffer of `n` copies of `px`.
    fn uniform_positive(px: [f32; 3], n: usize) -> Vec<f32> {
        px.iter().copied().cycle().take(3 * n).collect()
    }

    #[test]
    fn explicit_print_gains_pass_through() {
        // Explicit is a pass-through: no statistics run, the image is ignored.
        let gains = [1.3, 1.0, 0.7];
        assert_eq!(
            resolve_print_gains(&[], WbSource::Explicit(gains)).unwrap(),
            gains
        );
    }

    #[test]
    fn gray_world_gains_neutralize_a_uniform_cast() {
        // Every pixel carries the same cast, so the trimmed means are exactly the
        // cast and the gains are the green-anchored inverse.
        let rgb = uniform_positive([0.4, 0.5, 0.8], 200);
        let gains = estimate_gains(&rgb, Estimator::GrayWorld).unwrap();
        assert!(approx(gains[0], 0.5 / 0.4, 1e-5), "r gain {}", gains[0]);
        assert_eq!(gains[1], 1.0, "green-anchored");
        assert!(approx(gains[2], 0.5 / 0.8, 1e-5), "b gain {}", gains[2]);
        for px in rgb.as_chunks::<3>().0 {
            let balanced = [px[0] * gains[0], px[1] * gains[1], px[2] * gains[2]];
            assert!(approx(balanced[0], balanced[1], 1e-5));
            assert!(approx(balanced[1], balanced[2], 1e-5));
        }
    }

    #[test]
    fn percentile_gains_equalize_near_white_levels() {
        // Channels are the same ramp scaled per channel, so every per-channel
        // statistic scales with it and both modes recover the inverse scale.
        let scale = [0.8f32, 1.0, 1.2];
        let mut rgb = Vec::new();
        for i in 0..100 {
            let t = (i + 1) as f32 / 100.0;
            rgb.extend_from_slice(&[scale[0] * t, scale[1] * t, scale[2] * t]);
        }
        for mode in BOTH {
            let gains = estimate_gains(&rgb, mode).unwrap();
            assert!(approx(gains[0], 1.0 / 0.8, 1e-4), "{mode:?} r {}", gains[0]);
            assert_eq!(gains[1], 1.0, "{mode:?} green-anchored");
            assert!(approx(gains[2], 1.0 / 1.2, 1e-4), "{mode:?} b {}", gains[2]);
        }
    }

    #[test]
    fn percentile_mode_resists_a_dominant_color_gray_world_does_not() {
        // 90% of the frame is a strong green subject; 10% is genuinely neutral
        // near-white. The percentile lands on the neutral highlights, while the
        // gray-world means are dragged by the subject.
        let mut rgb = uniform_positive([0.2, 0.6, 0.2], 90);
        rgb.extend(uniform_positive([0.9, 0.9, 0.9], 10));
        let p = estimate_gains(&rgb, Estimator::Percentile).unwrap();
        for (c, gain) in p.into_iter().enumerate() {
            assert!(approx(gain, 1.0, 1e-5), "percentile chan {c}: {gain}");
        }
        let gw = estimate_gains(&rgb, Estimator::GrayWorld).unwrap();
        assert!(
            gw[0] > 2.0,
            "gray-world red gain dragged by cast: {}",
            gw[0]
        );
    }

    #[test]
    fn estimates_ignore_non_finite_and_extreme_samples() {
        // A NaN sample, an inf sample, and a huge finite outlier (< 1% of the
        // data) must not move either statistic off the bulk values.
        let mut rgb = uniform_positive([0.4, 0.5, 0.6], 200);
        rgb.extend_from_slice(&[1000.0, f32::NAN, f32::INFINITY]);
        for mode in BOTH {
            let gains = estimate_gains(&rgb, mode).unwrap();
            assert!(approx(gains[0], 0.5 / 0.4, 1e-3), "{mode:?} r {}", gains[0]);
            assert!(approx(gains[2], 0.5 / 0.6, 1e-3), "{mode:?} b {}", gains[2]);
        }
    }

    #[test]
    fn estimates_fail_loudly_on_an_unusable_channel() {
        // An all-non-finite channel has no usable level — a loud error (exit 1),
        // never silently-neutral or garbage gains.
        let rgb = uniform_positive([f32::NAN, 0.5, 0.5], 8);
        for mode in BOTH {
            let err = estimate_gains(&rgb, mode).unwrap_err();
            assert_eq!(err.exit_code(), 1, "{mode:?}");
        }
        // A non-positive level is rejected the same way — reachable on the new
        // chain, whose wide-gamut linear samples can be negative.
        let rgb = uniform_positive([0.0, 0.5, 0.5], 8);
        for mode in BOTH {
            assert!(estimate_gains(&rgb, mode).is_err(), "{mode:?}");
        }
    }

    #[test]
    fn auto_wb_stride_is_bounded() {
        assert_eq!(auto_wb_stride(0), 1);
        assert_eq!(auto_wb_stride(AUTO_WB_MAX_PIXELS), 1);
        let big = 7 * AUTO_WB_MAX_PIXELS + 3;
        let stride = auto_wb_stride(big);
        assert!(big.div_ceil(stride) <= AUTO_WB_MAX_PIXELS);
    }

    #[test]
    fn a_region_sample_reads_only_the_region() {
        // A 4x3 frame whose pixel value encodes its own position; the 2x2 region at
        // (1, 1) must yield exactly those four pixels, row-major.
        let (width, height) = (4u32, 3u32);
        let rgb: Vec<f32> = (0..width * height)
            .flat_map(|i| [i as f32, 0.0, 0.0])
            .collect();
        let sampled = sample_region(&rgb, width, [1, 1, 2, 2]).unwrap();
        let reds: Vec<f32> = sampled.as_chunks::<3>().0.iter().map(|p| p[0]).collect();
        assert_eq!(reds, [5.0, 6.0, 9.0, 10.0]);
    }

    #[test]
    fn a_whole_frame_region_samples_what_the_frame_sample_does() {
        // The two samplers agree when the region is the frame, so the region path
        // introduces no second convention for striding.
        let (width, height) = (5u32, 4u32);
        let rgb: Vec<f32> = (0..width * height * 3).map(|i| i as f32).collect();
        assert_eq!(
            sample_region(&rgb, width, [0, 0, width, height]).unwrap(),
            sample_frame(&rgb)
        );
    }

    #[test]
    fn a_region_outside_the_frame_is_refused() {
        let rgb = vec![0.5; 4 * 3 * 3];
        for region in [[0, 0, 0, 2], [0, 0, 2, 0], [3, 0, 2, 1], [0, 2, 1, 2]] {
            assert!(
                sample_region(&rgb, 4, region).is_err(),
                "{region:?} must be refused"
            );
        }
    }
}
