//! **The roll white balance** (`nf-scene-correction/roll-white-balance`): one set of
//! white-balance gains per roll, measured over the roll's own picture frames and
//! frozen into the recipe as `scene_correction.white_balance`.
//!
//! It removes what a whole roll shares — the film, development and scanner cast —
//! and keeps the scene's light: one sunset frame barely moves a roll statistic,
//! where a per-frame estimate would remove the sunset
//! (`docs/spike/desaturation-band.md`). Rendering never runs this; `measure-roll`
//! does, once, and every frame then reads the stated gains.
//!
//! Pure statistics over samples the orchestrator takes at the **decode's output in
//! linear ACEScg** — the input scene correction's gains multiply. The look's
//! contrast is a channel-equal power pivoted at mid-grey, so a white these gains
//! make neutral stays neutral under any contrast; the gain *values* belong to this
//! decode, which is why they are measured here and not at a rendered output.

use serde::Serialize;

use crate::pipeline::white_balance::{green_anchored_gains, percentile_levels, sample_region};
use crate::types::{NcError, Result};

/// The per-channel percentile of the roll's pooled pixels taken as its white.
///
/// Measured against the gains the user's marked whites ask for (four rolls,
/// 2026-09-23): p99 lands within 0.03 of them on Gold200 and Ektar100. At p95–p97
/// a roll's top few percent is sky rather than white — Ektar100's blue gain comes
/// out 0.79–0.95 against the whites' 1.21.
pub const PERCENTILE: f32 = 0.99;

/// How close to the leader's density, on any channel, a pixel may come before it is
/// left out: a fully exposed frame mixed into the roll otherwise **is** the roll's top
/// percentile. Measured: the leader's own pixels added as a frame moved the gains
/// 0.4–1.3 stops without the guard, and not at all with it.
pub const LEADER_GUARD_DENSITY: f32 = 0.1;

/// Most pixels one frame contributes, so every frame weighs alike in the pool and a
/// long roll stays small in memory (~1.5 MB per frame). Against a dense sample the
/// gains moved ≤ 0.0023 stops on four rolls.
pub const FRAME_SAMPLE_PIXELS: usize = 1 << 17;

/// The leader measurement the guard reads, **written fresh** — the retiring
/// leader-`Dmax` anchor is not reused (`nf-retire/dmax-machinery`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct LeaderGuard {
    /// Per-channel median of the leader's centre, at the decode's output.
    pub median: [f32; 3],
    /// The guard, in density: [`LEADER_GUARD_DENSITY`].
    pub guard_density: f32,
    /// A pixel reaching this on any channel is left out:
    /// `median_c · 10^(−linearization · guard_density)`.
    pub ceiling: [f32; 3],
}

/// The centre half of a `width` x `height` leader: a leader is a uniform field, and
/// its edges are where the holder and any fogging gradient sit.
fn leader_region(width: u32, height: u32) -> [u32; 4] {
    [
        width / 4,
        height / 4,
        (width / 2).max(1),
        (height / 2).max(1),
    ]
}

/// Measure the guard from a decoded leader (`rgb`, `width` x `height`, at the
/// decode's output), over its centre half, at a decode of the given `linearization`
/// (`reconstruction.linearization` — the decode's slope, not the look's contrast).
///
/// The guard is stated in density because that is what the leader's nearness means,
/// but it is applied to linear ACEScg, where the decode's `10^(linearization·D′)` makes a
/// density step a ratio. The working-space 3×3 mixes the channels a little, so on a
/// saturated pixel the step is approximate — it only has to separate a fully exposed
/// frame from picture content, which sit well apart.
pub fn leader_guard(
    rgb: &[f32],
    width: u32,
    height: u32,
    linearization: f32,
) -> Result<LeaderGuard> {
    let sample = sample_region(
        rgb,
        width,
        leader_region(width, height),
        FRAME_SAMPLE_PIXELS,
    )?;
    let median = percentile_levels(&sample, 0.5);
    for (m, name) in median.iter().zip(["red", "green", "blue"]) {
        if !m.is_finite() || *m <= 0.0 {
            return Err(NcError::Other(format!(
                "leader: the {name} channel has no usable level (got {m}) — is the file \
                 a fully exposed leader, decoded with the roll's film base?"
            )));
        }
    }
    let ratio = 10f32.powf(-linearization * LEADER_GUARD_DENSITY);
    Ok(LeaderGuard {
        median,
        guard_density: LEADER_GUARD_DENSITY,
        ceiling: median.map(|m| m * ratio),
    })
}

/// What one frame gave the pool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct FrameCounts {
    /// Pixels sampled from the frame's measurement region.
    pub sampled: usize,
    /// Pooled.
    pub kept: usize,
    /// Left out by the leader guard.
    pub guarded: usize,
    /// Left out because a channel is non-finite or not positive — no gain can
    /// correct it, and it would drag a percentile.
    pub unusable: usize,
}

/// Sample one frame's measurement `region` and add what survives to `pool`.
pub fn pool_frame(
    rgb: &[f32],
    width: u32,
    region: [u32; 4],
    guard: Option<&LeaderGuard>,
    pool: &mut Vec<f32>,
) -> Result<FrameCounts> {
    let sample = sample_region(rgb, width, region, FRAME_SAMPLE_PIXELS)?;
    let mut counts = FrameCounts::default();
    for px in sample.as_chunks::<3>().0 {
        counts.sampled += 1;
        if px.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            counts.unusable += 1;
        } else if guard.is_some_and(|g| (0..3).any(|c| px[c] >= g.ceiling[c])) {
            counts.guarded += 1;
        } else {
            counts.kept += 1;
            pool.extend_from_slice(px);
        }
    }
    Ok(counts)
}

/// The roll's white-balance gains: green-anchored, equalizing the pooled pixels'
/// per-channel [`PERCENTILE`].
pub fn roll_gains(pool: &[f32]) -> Result<[f32; 3]> {
    if pool.is_empty() {
        return Err(NcError::Other(
            "roll white balance: no usable pixel survived on any frame — the frames \
             are unusable, or the leader guard left nothing (is the leader from this \
             roll?)"
                .into(),
        ));
    }
    green_anchored_gains(percentile_levels(pool, PERCENTILE), "roll white balance")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An interleaved buffer of `n` copies of `px`.
    fn field(px: [f32; 3], n: usize) -> Vec<f32> {
        px.iter().copied().cycle().take(3 * n).collect()
    }

    fn pooled(frames: &[Vec<f32>], guard: Option<&LeaderGuard>) -> Vec<f32> {
        let mut pool = Vec::new();
        for f in frames {
            let n = (f.len() / 3) as u32;
            pool_frame(f, n, [0, 0, n, 1], guard, &mut pool).unwrap();
        }
        pool
    }

    #[test]
    fn gains_invert_a_roll_wide_cast() {
        // Every frame is a ramp under the same cast; the percentile of each channel
        // scales with the cast, so the gains are its green-anchored inverse.
        let cast = [1.25f32, 1.0, 0.8];
        let frames: Vec<Vec<f32>> = (0..3)
            .map(|k| {
                (1..=200)
                    .flat_map(|i| {
                        let t = (i + k) as f32 / 200.0;
                        cast.map(|c| c * t)
                    })
                    .collect()
            })
            .collect();
        let gains = roll_gains(&pooled(&frames, None)).unwrap();
        assert!((gains[0] - 0.8).abs() < 1e-5, "{gains:?}");
        assert_eq!(gains[1], 1.0);
        assert!((gains[2] - 1.25).abs() < 1e-5, "{gains:?}");
    }

    #[test]
    fn the_guard_keeps_a_fully_exposed_frame_out_of_the_white() {
        // A roll of neutral content at 1.0, plus one frame at the leader's own level
        // with a strong cast. Unguarded, that frame *is* the top percentile; guarded,
        // the gains are the neutral roll's.
        let leader = [8.0f32, 6.0, 3.0];
        let picture = field([1.0, 1.0, 1.0], 1000);
        let blown = field(leader, 1000);
        let guard = leader_guard(&field(leader, 16), 16, 1, 2.0).unwrap();

        let frames = vec![picture.clone(), picture.clone(), blown];
        let unguarded = roll_gains(&pooled(&frames, None)).unwrap();
        let guarded = roll_gains(&pooled(&frames, Some(&guard))).unwrap();
        assert_ne!(
            unguarded,
            [1.0, 1.0, 1.0],
            "the blown frame must move it unguarded"
        );
        assert_eq!(guarded, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn the_guard_sits_the_stated_density_below_the_leader() {
        let guard = leader_guard(&field([2.0, 1.0, 0.5], 9), 9, 1, 2.0).unwrap();
        let ratio = 10f32.powf(-2.0 * LEADER_GUARD_DENSITY);
        assert_eq!(guard.median, [2.0, 1.0, 0.5]);
        assert_eq!(guard.ceiling, [2.0 * ratio, ratio, 0.5 * ratio]);
    }

    #[test]
    fn counts_account_for_every_sampled_pixel() {
        let guard = leader_guard(&field([2.0, 2.0, 2.0], 4), 4, 1, 2.0).unwrap();
        let mut frame = field([0.5, 0.5, 0.5], 5); // kept
        frame.extend([f32::NAN, 0.5, 0.5, 0.0, 0.5, 0.5]); // two unusable
        frame.extend(field([1.9, 0.5, 0.5], 3)); // guarded on red
        let mut pool = Vec::new();
        let counts = pool_frame(&frame, 10, [0, 0, 10, 1], Some(&guard), &mut pool).unwrap();
        assert_eq!(
            counts,
            FrameCounts {
                sampled: 10,
                kept: 5,
                guarded: 3,
                unusable: 2
            }
        );
        assert_eq!(pool.len(), 15);
    }

    #[test]
    fn the_leader_is_read_at_its_centre_not_its_edges() {
        // A 4x4 leader whose outer ring is the holder (dark) and whose centre is the
        // exposed field: the guard must sit on the field.
        let (w, h) = (4u32, 4u32);
        let rgb: Vec<f32> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                let centre = (1..3).contains(&x) && (1..3).contains(&y);
                if centre {
                    [3.0, 2.0, 1.0]
                } else {
                    [0.01, 0.01, 0.01]
                }
            })
            .collect();
        assert_eq!(
            leader_guard(&rgb, w, h, 2.0).unwrap().median,
            [3.0, 2.0, 1.0]
        );
    }

    #[test]
    fn an_empty_pool_is_refused_not_neutral() {
        let err = roll_gains(&[]).unwrap_err();
        assert!(err.message().contains("no usable pixel"), "{err}");
    }

    #[test]
    fn a_leader_with_no_usable_channel_is_refused() {
        let err = leader_guard(&field([1.0, 0.0, 1.0], 4), 4, 1, 2.0).unwrap_err();
        assert!(err.message().contains("green"), "{err}");
    }

    #[test]
    fn every_frame_over_the_cap_contributes_the_same_count() {
        // Equal weight in the pool, whatever the frame's size — including just past the
        // cap, where an integer stride would halve the sample.
        for n in [
            FRAME_SAMPLE_PIXELS,
            FRAME_SAMPLE_PIXELS + 1,
            FRAME_SAMPLE_PIXELS * 3 + 7,
        ] {
            let frame = field([0.5, 0.5, 0.5], n);
            let mut pool = Vec::new();
            let counts =
                pool_frame(&frame, n as u32, [0, 0, n as u32, 1], None, &mut pool).unwrap();
            assert_eq!(counts.sampled, FRAME_SAMPLE_PIXELS, "{n}: {counts:?}");
        }
    }
}
