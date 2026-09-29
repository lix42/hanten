//! White-balance statistics: deterministic per-channel levels and the
//! green-anchored gains that equalize them.
//!
//! One consumer: the roll white balance (`pipeline::roll_white`), which samples each
//! frame's measurement region and pools the samples over a roll. There is no per-frame
//! estimate (`nf-scene-correction/roll-white-balance` retired it).
//!
//! Pure statistics, no ML (the project's "AI-friendly ≠ ML" rule): same sample ⇒
//! identical gains.

use crate::types::{NcError, Result};

/// Sample the rectangle `[x, y, w, h]` of a `width`-wide interleaved-RGB frame:
/// every pixel when it has at most `max_pixels`, otherwise exactly `max_pixels`
/// spread evenly over the region's own row-major pixel index — so the sample depends
/// on the region, never on the frame around it.
///
/// A fixed count rather than an integer stride: a stride halves the sample the
/// moment a region passes a multiple of the cap (`max_pixels + 1` pixels would keep
/// half as many as `max_pixels`), and the roll pool weighs frames by their samples.
///
/// Fails on a region that is empty or leaves the frame: a caller that reaches
/// here with one has resolved the wrong rectangle, and sampling what remains
/// would estimate over pixels nobody chose.
pub(crate) fn sample_region(
    rgb: &[f32],
    width: u32,
    region: [u32; 4],
    max_pixels: usize,
) -> Result<Vec<f32>> {
    let [x, y, w, h] = region.map(|v| v as usize);
    let width = width as usize;
    let height = (rgb.len() / 3).checked_div(width).unwrap_or(0);
    if w == 0 || h == 0 || x + w > width || y + h > height {
        return Err(NcError::Other(format!(
            "white balance: the measurement region {region:?} is empty or leaves \
             the {width}x{height} frame"
        )));
    }
    let pixels = w * h;
    let count = pixels.min(max_pixels);
    let mut sampled = Vec::with_capacity(count * 3);
    for k in 0..count {
        // `k · pixels / count` in u128: exact, monotone, and `< pixels` for `k < count`.
        let i = (k as u128 * pixels as u128 / count as u128) as usize;
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

/// Nearest-rank percentile of a sorted, non-empty slice (`round((n−1)·p)`).
fn nearest_rank(sorted: &[f32], p: f32) -> f32 {
    sorted[nearest_rank_index(sorted.len(), p)]
}

/// The index [`nearest_rank`] reads in a sorted slice of `len > 0` — shared with
/// `roll_white`'s white, so its p97 and the gains' p99 round alike.
pub(crate) fn nearest_rank_index(len: usize, p: f32) -> usize {
    (((len - 1) as f32) * p).round() as usize
}

/// The remedy when an estimate fails, as flag and recipe key.
const STATE_GAINS_INSTEAD: &str = "state explicit gains instead (`--white-balance` on \
     `convert` or `roll`, or the recipe's `scene_correction.white_balance` as \
     `{\"explicit\": [r, g, b]}`)";

/// Per-channel nearest-rank percentile `p` of an **already-sampled** `rgb` over its
/// finite samples; `NaN` for a channel with none. The caller judges usability, via
/// [`green_anchored_gains`].
pub(crate) fn percentile_levels(rgb: &[f32], p: f32) -> [f32; 3] {
    channel_levels(rgb, |sorted| nearest_rank(sorted, p))
}

/// Per-channel `level(sorted finite samples)`; `NaN` for a channel with none — the
/// one place an estimator's empty-channel rule lives.
fn channel_levels(rgb: &[f32], level: impl Fn(&[f32]) -> f32) -> [f32; 3] {
    let channels = wb_channel_samples(rgb);
    std::array::from_fn(|c| {
        if channels[c].is_empty() {
            f32::NAN
        } else {
            level(&channels[c])
        }
    })
}

/// The gains `[g/r, 1, g/b]` that equalize per-channel `level`s — green-anchored,
/// because white balance corrects colour and brightness is exposure's job.
///
/// Fails loudly ([`NcError::Other`], exit 1) when a level is unusable (non-finite
/// or non-positive, which no multiplicative gain corrects) or a gain comes out
/// non-finite, naming `what` measured it — never silently-neutral or garbage gains.
pub(crate) fn green_anchored_gains(level: [f32; 3], what: &str) -> Result<[f32; 3]> {
    for (l, name) in level.into_iter().zip(["red", "green", "blue"]) {
        if !l.is_finite() || l <= 0.0 {
            return Err(NcError::Other(format!(
                "{what}: the {name} channel has no usable level (got {l}); \
                 {STATE_GAINS_INSTEAD}"
            )));
        }
    }
    let gains = [level[1] / level[0], 1.0, level[1] / level[2]];
    for (g, name) in gains.into_iter().zip(["red", "green", "blue"]) {
        // Positive finite levels can still divide into inf/0 across an extreme
        // dynamic range (subnormal denominators); guard the gains themselves.
        if !g.is_finite() || g <= 0.0 {
            return Err(NcError::Other(format!(
                "{what}: estimated {name} gain is not a positive finite value \
                 (got {g}); {STATE_GAINS_INSTEAD}"
            )));
        }
    }
    Ok(gains)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sample cap no test region reaches.
    const CAP: usize = 1 << 20;

    /// An interleaved RGB buffer of `n` copies of `px`.
    fn uniform_positive(px: [f32; 3], n: usize) -> Vec<f32> {
        px.iter().copied().cycle().take(3 * n).collect()
    }

    #[test]
    fn estimates_fail_loudly_on_an_unusable_channel() {
        // An all-non-finite channel has no usable level — a loud error (exit 1),
        // never silently-neutral or garbage gains.
        let gains = |rgb: &[f32]| green_anchored_gains(percentile_levels(rgb, 0.99), "test");
        let err = gains(&uniform_positive([f32::NAN, 0.5, 0.5], 8)).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        // A non-positive level is rejected the same way — reachable on the chain,
        // whose wide-gamut linear samples can be negative.
        assert!(gains(&uniform_positive([0.0, 0.5, 0.5], 8)).is_err());
    }

    #[test]
    fn a_region_sample_reads_only_the_region() {
        // A 4x3 frame whose pixel value encodes its own position; the 2x2 region at
        // (1, 1) must yield exactly those four pixels, row-major.
        let (width, height) = (4u32, 3u32);
        let rgb: Vec<f32> = (0..width * height)
            .flat_map(|i| [i as f32, 0.0, 0.0])
            .collect();
        let sampled = sample_region(&rgb, width, [1, 1, 2, 2], CAP).unwrap();
        let reds: Vec<f32> = sampled.as_chunks::<3>().0.iter().map(|p| p[0]).collect();
        assert_eq!(reds, [5.0, 6.0, 9.0, 10.0]);
    }

    #[test]
    fn a_region_outside_the_frame_is_refused() {
        let rgb = vec![0.5; 4 * 3 * 3];
        for region in [[0, 0, 0, 2], [0, 0, 2, 0], [3, 0, 2, 1], [0, 2, 1, 2]] {
            assert!(
                sample_region(&rgb, 4, region, CAP).is_err(),
                "{region:?} must be refused"
            );
        }
    }
}
