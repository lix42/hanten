//! **The roll white balance** (`nf-scene-correction/roll-white-balance`): one set of
//! white-balance gains per roll, measured over the roll's own picture frames and
//! frozen into the recipe as `roll.white_balance` (`nf-calibration/roll-section`), which
//! scene correction's own white balance then multiplies.
//!
//! It removes what a whole roll shares — the film, development and scanner cast —
//! and keeps the scene's light: one sunset frame barely moves a roll statistic,
//! where a per-frame estimate would remove the sunset
//! (`docs/spike/desaturation-band.md`). Rendering never runs this; `measure-roll`
//! does, once, and every frame then reads the stated gains.
//!
//! Pure statistics over samples the orchestrator takes at the **decode's output in
//! linear ACEScg** — the input scene correction's gains multiply. The look's
//! slope is a channel-equal power pivoted at mid-grey, so a white these gains
//! make neutral stays neutral under any slope; the gain *values* belong to this
//! decode, which is why they are measured here and not at a rendered output.
//!
//! **The roll's white** (`nf-calibration/roll-white-rule`) is measured from the same
//! per-frame samples and placed through the look's slope, with mid-grey pinned, so the
//! roll's white renders at diffuse white. The recipe stores the white
//! (`roll.white_stops`), not the slope: [`slope_for`] is applied at render time, and
//! `look.contrast` multiplies it. The rule was chosen by review on nine rolls
//! (`nf-calibration/anchor-comparison`), with a black point in the chain; its values are
//! provisional, since that sample held no deliberately bad frames:
//!
//! - A frame's white is the [`WHITE_PERCENTILE`] of its pixels' **brightest channel**
//!   over its effective area, which keeps the specular headroom above white, in **scene
//!   stops** above mid-grey — on the decode's film RGB, before the working-space 3×3
//!   ([`frame_white`]).
//! - The roll's white is the brightest frame white at or under [`WHITE_CAP_STOPS`],
//!   raised to at least [`WHITE_FLOOR_STOPS`]; if every frame is above the cap, it is the
//!   cap. A frame above the cap is **clamped**: it renders at the cap's slope, not the
//!   roll's, and the report discloses it — an ordinary bright scene is clamped too, so it
//!   is not a warning.
//! - A frame whose white is within [`SATURATION_MARGIN_STOPS`] of the leader is near film
//!   saturation, and that warns. Without a leader there is no check.
//!
//! **A frame's white is measured before the leader guard.** The guard drops pixels
//! within [`LEADER_GUARD_DENSITY`] (≈0.6 stop) of the leader — exactly the highlights of
//! a frame near saturation. Measured after it, such a frame's white lands lower, can
//! escape the clamp and set the roll's slope, and can never come within the margin of
//! its leader. The cap already keeps a blown frame from raising the roll's white, so the
//! guard serves the white balance only; a frame it empties is also left out of the
//! exposure.
//!
//! **The two measurements take different domains from one decode**: the gains are
//! measured in ACEScg, where they multiply; the white in film RGB, the domain the rule was
//! reviewed in. The orchestrator reads the film RGB before mapping it.
//!
//! **The roll's exposure** (`nf-calibration/roll-exposure`) lifts an under-exposed roll,
//! which the white rule cannot: it pins mid-grey and only moves contrast. It is one
//! neutral gain for the whole roll (`roll.exposure`, added to scene correction's), so a
//! frame darker than its roll stays dark. Each frame's level is the log-average of its
//! luma in ACEScg ([`frame_level`]); the roll's exposure brings the median frame level to
//! [`LEVEL_TARGET_STOPS`], within [`EXPOSURE_BOUND_EV`] ([`roll_exposure`]). A median over
//! frames, so one night scene cannot set it — the failure that retired per-frame auto
//! white balance. **The white is measured at exposure 0**: re-placing it after the
//! exposure was not better in review, and a small exposure could push one frame over the
//! cap and move the roll's slope by a third.

use serde::Serialize;

use crate::algo::fixed::DIFFUSE_WHITE;
use crate::pipeline::colorimetry::pinned::ACESCG_LUMA;
use crate::pipeline::look::MID_GREY;
use crate::pipeline::white_balance::{
    green_anchored_gains, nearest_rank_index, percentile_levels, sample_region,
};
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

/// The percentile of a frame's usable pixels taken as its white. The review measured it
/// (`nf-calibration/anchor-comparison`); p97 leaves a typical frame's speculars above
/// white rather than at it.
pub const WHITE_PERCENTILE: f32 = 0.97;

/// The brightest a roll's white may sit, in scene stops above mid-grey; a frame above it
/// is clamped to it. Chosen by review over +2.5 and +3.0 (flatter).
pub const WHITE_CAP_STOPS: f32 = 2.0;

/// The dimmest a roll's white may sit, in scene stops above mid-grey: the contrast lifts
/// an underexposed roll's white only this far (its level is the roll's exposure's).
/// Chosen by review over +1.0 (contrast 4.45, worst on nearly every frame) and +2.0.
pub const WHITE_FLOOR_STOPS: f32 = 1.5;

/// How close to the leader, in scene stops, a frame's white may come before the frame
/// warns as near film saturation. A placeholder, owned by
/// `nf-calibration/saturation-margin`: set from one frame judged overexposed. A leader
/// can sit below the film's shoulder, so a frame near it need not be saturated.
pub const SATURATION_MARGIN_STOPS: f32 = 0.5;

/// Where a roll's exposure puts its median frame level ([`frame_level`]), in scene stops
/// from mid-grey. Chosen by review over -1.0 and -0.8 (both lost on nearly every frame)
/// and -0.3 (split frame by frame: low-key frames wanted it, bright ones did not).
pub const LEVEL_TARGET_STOPS: f32 = -0.6;

/// The most a measured roll exposure moves, either way, in EV. The ten rolls reviewed
/// measured +0.02 to +1.74.
pub const EXPOSURE_BOUND_EV: f32 = 2.0;

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
        if unusable(px) {
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

/// A pixel no gain corrects and no percentile should see: a channel non-finite or not
/// positive.
fn unusable(px: &[f32; 3]) -> bool {
    px.iter().any(|v| !v.is_finite() || *v <= 0.0)
}

/// Each usable pixel's brightest channel, unordered.
fn usable_peaks(sample: &[f32]) -> Vec<f32> {
    sample
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|px| !unusable(px))
        .map(|px| px[0].max(px[1]).max(px[2]))
        .collect()
}

/// The nearest-rank percentile `p` of a non-empty slice, by selection rather than a full
/// sort: the element a sort would put at [`nearest_rank_index`], since `total_cmp` is a
/// total order.
fn nearest_rank_of(mut values: Vec<f32>, p: f32) -> f32 {
    let at = nearest_rank_index(values.len(), p);
    *values.select_nth_unstable_by(at, f32::total_cmp).1
}

/// A frame's white: the [`WHITE_PERCENTILE`] of its usable pixels' brightest channel over
/// `region`, **guard or no guard**; `None` when no pixel is usable.
///
/// **The brightest channel, not one channel**, so a highlight reads bright whatever its
/// colour. The review measured red; on a blue- or green-lit highlight red under-reads —
/// on two Ektar rolls, by 0.5–2 stops on most frames — and the solved contrast then
/// pushes that highlight past white. On a neutral or warm highlight the two agree, so the
/// reviewed cap and floor keep their meaning (the roll's white moved ≤ 0.2 stop on nine
/// rolls). A saturated coloured highlight now sets the white; highlight desaturation
/// leaves such a pixel's colour alone (`pipeline::look`).
///
/// `rgb` is the decode's **film RGB**, before the working-space 3×3 — the domain the rule
/// was reviewed in. The ACEScg values after the 3×3 mix the channels, and with them the
/// roll's uncorrected cast: measured there, red moved frames −0.5 to +0.6 stop, by stock,
/// off the reviewed values.
pub fn frame_white(rgb: &[f32], width: u32, region: [u32; 4]) -> Result<Option<f32>> {
    let peaks = usable_peaks(&sample_region(rgb, width, region, FRAME_SAMPLE_PIXELS)?);
    Ok((!peaks.is_empty()).then(|| nearest_rank_of(peaks, WHITE_PERCENTILE)))
}

/// A frame's level: the log-average luma over `region` of its pixels with finite channels
/// and positive luma — an out-of-gamut pixel with a negative channel still counts — in
/// scene stops from mid-grey; `None` when no pixel counts. **No leader guard**: it would
/// drop the real highlights of a frame near saturation. `rgb` is the decode's **linear
/// ACEScg** — where the roll's exposure is a gain, so an exposure of `e` moves the level
/// by exactly `e` — before white balance, whose green-anchored gains move it a few
/// hundredths of a stop. Summed in f64 in sample order, so it is deterministic.
pub fn frame_level(rgb: &[f32], width: u32, region: [u32; 4]) -> Result<Option<f32>> {
    let sample = sample_region(rgb, width, region, FRAME_SAMPLE_PIXELS)?;
    let (sum, n) = sample
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|px| px.iter().all(|v| v.is_finite()))
        .map(|px| (0..3).map(|c| ACESCG_LUMA[c] * px[c]).sum::<f32>())
        .filter(|y| *y > 0.0)
        .fold((0.0f64, 0usize), |(s, n), y| {
            (s + f64::from(y).log2(), n + 1)
        });
    Ok((n > 0).then(|| (sum / n as f64) as f32 - MID_GREY.log2()))
}

/// The roll's exposure, from each frame's level.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RollExposure {
    /// The gain, in EV: what `roll.exposure` stores.
    pub ev: f32,
    /// The median frame level it was measured from, in scene stops.
    pub level_stops: f32,
    /// Whether [`EXPOSURE_BOUND_EV`] limited it.
    pub bounded: bool,
}

/// The exposure that brings the median of the frames' levels ([`frame_level`]; `None`
/// for a frame with no usable pixel, or one the leader guard emptied) to [`LEVEL_TARGET_STOPS`], within
/// [`EXPOSURE_BOUND_EV`]. An even count takes the mean of the middle two.
pub fn roll_exposure(levels: &[Option<f32>]) -> Result<RollExposure> {
    let mut known: Vec<f32> = levels.iter().flatten().copied().collect();
    if known.is_empty() {
        return Err(NcError::Other(
            "roll exposure: no frame has a usable pixel, so the roll has no level".into(),
        ));
    }
    known.sort_by(f32::total_cmp);
    let mid = known.len() / 2;
    let level_stops = if known.len().is_multiple_of(2) {
        (known[mid - 1] + known[mid]) / 2.0
    } else {
        known[mid]
    };
    let wanted = LEVEL_TARGET_STOPS - level_stops;
    let ev = wanted.clamp(-EXPOSURE_BOUND_EV, EXPOSURE_BOUND_EV);
    Ok(RollExposure {
        ev,
        level_stops,
        bounded: ev != wanted,
    })
}

/// The median of the leader's brightest channel over its centre half, in the same
/// film-RGB measure as [`frame_white`] — what a frame's saturation distance is measured
/// against; `None` when no pixel is usable. The caller refuses that only after
/// [`leader_guard`] has had the chance to name the empty channel.
pub fn leader_peak(rgb: &[f32], width: u32, height: u32) -> Result<Option<f32>> {
    let peaks = usable_peaks(&sample_region(
        rgb,
        width,
        leader_region(width, height),
        FRAME_SAMPLE_PIXELS,
    )?);
    Ok((!peaks.is_empty()).then(|| nearest_rank_of(peaks, 0.5)))
}

/// A linear level at the decode's output in **scene stops** above mid-grey: the anchor
/// rule pins mid-grey at [`MID_GREY`], and the decode's output is linear in scene light.
/// On film red this is the reviewed `(D − d) · linearization / log10 2`.
pub fn scene_stops(level: f32) -> f32 {
    (level / MID_GREY).log2()
}

/// The slope that renders a white `white_stops` above mid-grey at diffuse white,
/// mid-grey pinned: the look maps `MID_GREY · 2^w` to `MID_GREY · 2^(k·w)`. Scene stops
/// already include the decode's linearization, so it does not enter here.
pub fn slope_for(white_stops: f32) -> f32 {
    (DIFFUSE_WHITE / MID_GREY).log2() / white_stops
}

/// Which limit set the roll's white.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WhiteBound {
    /// A frame's own white, between the floor and the cap.
    None,
    /// The brightest frame at or under the cap sat below the floor.
    Floor,
    /// Every frame sat above the cap.
    Cap,
}

/// A frame's part in the roll's white.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameRole {
    /// The roll's white is this frame's white.
    SetsRoll,
    /// At or under the cap, and not the roll's white (or the floor raised it).
    Under,
    /// Above the cap: rendered at the cap's slope, not the roll's.
    Clamped,
    /// No usable pixel, so no white.
    Unmeasured,
}

/// The roll's white, placed by the rule.
#[derive(Clone, Debug, PartialEq)]
pub struct RollWhite {
    /// In scene stops above mid-grey.
    pub stops: f32,
    pub bound: WhiteBound,
    /// Per frame, in input order.
    pub roles: Vec<FrameRole>,
}

/// Place the roll's white from each frame's white in scene stops (`None` for a frame
/// with no usable pixel): the brightest at or under [`WHITE_CAP_STOPS`], raised to at
/// least [`WHITE_FLOOR_STOPS`]; the cap when every frame is above it.
pub fn place_roll_white(frame_stops: &[Option<f32>]) -> Result<RollWhite> {
    let over = |w: f32| w > WHITE_CAP_STOPS;
    // The first of equal whites, so the choice follows input order.
    let brightest_under = frame_stops
        .iter()
        .enumerate()
        .filter_map(|(i, w)| w.filter(|w| !over(*w)).map(|w| (i, w)))
        .fold(None, |best: Option<(usize, f32)>, (i, w)| match best {
            Some((_, b)) if b >= w => best,
            _ => Some((i, w)),
        });
    let (stops, bound, set_by) = match brightest_under {
        Some((i, w)) if w >= WHITE_FLOOR_STOPS => (w, WhiteBound::None, Some(i)),
        Some(_) => (WHITE_FLOOR_STOPS, WhiteBound::Floor, None),
        None if frame_stops.iter().any(Option::is_some) => (WHITE_CAP_STOPS, WhiteBound::Cap, None),
        None => {
            return Err(NcError::Other(
                "roll white: no frame has a usable pixel, so the roll has no white".into(),
            ));
        }
    };
    let roles = frame_stops
        .iter()
        .enumerate()
        .map(|(i, w)| match w {
            None => FrameRole::Unmeasured,
            Some(w) if over(*w) => FrameRole::Clamped,
            Some(_) if set_by == Some(i) => FrameRole::SetsRoll,
            Some(_) => FrameRole::Under,
        })
        .collect();
    Ok(RollWhite {
        stops,
        bound,
        roles,
    })
}

/// How far a frame's white sits under the leader ([`leader_peak`]), in scene stops.
/// Negative when the white is above the leader.
pub fn leader_distance_stops(white: f32, leader_peak: f32) -> f32 {
    (leader_peak / white).log2()
}

/// Whether a frame whose white sits `distance` stops under its leader is near film
/// saturation.
pub fn near_saturation(distance: f32) -> bool {
    distance < SATURATION_MARGIN_STOPS
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
    fn a_frames_level_is_its_log_average_luma() {
        // Half the pixels a stop over mid-grey, half a stop under: the log-average is
        // mid-grey, where the arithmetic mean would sit a quarter-stop over it.
        let mut rgb = field([0.36, 0.36, 0.36], 50);
        rgb.extend(field([0.09, 0.09, 0.09], 50));
        let level = frame_level(&rgb, 100, [0, 0, 100, 1]).unwrap().unwrap();
        assert!(level.abs() < 1e-5, "{level}");
        // A pixel with no positive luma, or a non-finite channel, is left out, not read as
        // black.
        rgb.extend(field([-0.5, 0.0, 0.0], 10));
        rgb.extend(field([f32::NAN, 0.36, 0.36], 10));
        let level = frame_level(&rgb, 120, [0, 0, 120, 1]).unwrap().unwrap();
        assert!(level.abs() < 1e-5, "{level}");
        assert_eq!(
            frame_level(&field([0.0; 3], 4), 4, [0, 0, 4, 1]).unwrap(),
            None
        );
        // An out-of-gamut pixel whose luma is positive counts, negative channel and all.
        let px = [-0.05f32, 0.2, 0.2];
        let y: f32 = (0..3).map(|c| ACESCG_LUMA[c] * px[c]).sum();
        assert!(y > 0.0);
        let level = frame_level(&field(px, 4), 4, [0, 0, 4, 1])
            .unwrap()
            .unwrap();
        assert!(
            (level - (y.log2() - MID_GREY.log2())).abs() < 1e-5,
            "{level}"
        );
    }

    #[test]
    fn the_roll_exposure_brings_the_median_level_to_the_target() {
        // One night frame and one bright frame move the median by nothing.
        let e = roll_exposure(&[Some(-6.0), Some(-2.0), None, Some(-1.8), Some(3.0)]).unwrap();
        assert_eq!(e.level_stops, -1.9);
        assert!((e.ev - (LEVEL_TARGET_STOPS + 1.9)).abs() < 1e-6, "{e:?}");
        assert!(!e.bounded);
        // Both ways within the bound, and a bound that binds says so.
        let dark = roll_exposure(&[Some(-4.0)]).unwrap();
        assert_eq!((dark.ev, dark.bounded), (EXPOSURE_BOUND_EV, true));
        let bright = roll_exposure(&[Some(3.0)]).unwrap();
        assert_eq!((bright.ev, bright.bounded), (-EXPOSURE_BOUND_EV, true));
        assert!(roll_exposure(&[None, None]).is_err());
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

    /// A neutral level `stops` above mid-grey at the decode's output.
    fn at(stops: f32) -> f32 {
        MID_GREY * stops.exp2()
    }

    #[test]
    fn the_roll_takes_its_brightest_frame_under_the_cap() {
        let w = place_roll_white(&[Some(1.6), Some(1.9), Some(1.7)]).unwrap();
        assert_eq!(w.stops, 1.9);
        assert_eq!(w.bound, WhiteBound::None);
        assert_eq!(
            w.roles,
            [FrameRole::Under, FrameRole::SetsRoll, FrameRole::Under]
        );
    }

    #[test]
    fn a_frame_above_the_cap_is_clamped_not_the_roll_white() {
        let w = place_roll_white(&[Some(3.2), Some(1.8)]).unwrap();
        assert_eq!(w.stops, 1.8);
        assert_eq!(w.roles, [FrameRole::Clamped, FrameRole::SetsRoll]);
        // The cap itself is "at or under".
        let w = place_roll_white(&[Some(WHITE_CAP_STOPS), Some(1.8)]).unwrap();
        assert_eq!(w.stops, WHITE_CAP_STOPS);
        assert_eq!(w.roles[0], FrameRole::SetsRoll);
    }

    #[test]
    fn a_roll_of_one_dark_frame_lands_on_the_floor() {
        let w = place_roll_white(&[Some(0.4)]).unwrap();
        assert_eq!(w.stops, WHITE_FLOOR_STOPS);
        assert_eq!(w.bound, WhiteBound::Floor);
        assert_eq!(w.roles, [FrameRole::Under]);
        assert_eq!(slope_for(w.stops), slope_for(WHITE_FLOOR_STOPS));
    }

    #[test]
    fn a_roll_with_every_frame_above_the_cap_lands_on_the_cap() {
        let w = place_roll_white(&[Some(2.4), Some(3.1)]).unwrap();
        assert_eq!(w.stops, WHITE_CAP_STOPS);
        assert_eq!(w.bound, WhiteBound::Cap);
        assert_eq!(w.roles, [FrameRole::Clamped, FrameRole::Clamped]);
    }

    #[test]
    fn a_frame_with_no_white_takes_no_part_and_a_roll_of_none_is_refused() {
        let w = place_roll_white(&[None, Some(1.7)]).unwrap();
        assert_eq!(w.roles, [FrameRole::Unmeasured, FrameRole::SetsRoll]);
        let err = place_roll_white(&[None, None]).unwrap_err();
        assert!(err.message().contains("usable pixel"), "{err}");
    }

    #[test]
    fn the_contrast_renders_the_white_at_diffuse_white_with_mid_pinned() {
        // The reviewed values: whole contrast 2.23 at the cap and 2.97 at the floor, at
        // the decode's linearization 1.8.
        let lin = crate::algo::fixed::LINEARIZATION;
        assert!((slope_for(WHITE_CAP_STOPS) * lin - 2.23).abs() < 0.01);
        assert!((slope_for(WHITE_FLOOR_STOPS) * lin - 2.97).abs() < 0.01);
        for w in [WHITE_FLOOR_STOPS, 1.8, WHITE_CAP_STOPS] {
            let k = slope_for(w);
            let white = MID_GREY * (at(w) / MID_GREY).powf(k);
            assert!((white - DIFFUSE_WHITE).abs() < 1e-5, "{w}: {white}");
        }
    }

    #[test]
    fn a_frames_white_is_its_brightest_channels_percentile_in_scene_stops() {
        // A red ramp from mid-grey to +3 stops: p97 sits near +2.91.
        let n = 1001;
        let frame: Vec<f32> = (0..n)
            .flat_map(|i| {
                let r = at(3.0 * i as f32 / (n - 1) as f32);
                [r, 0.18, 0.18]
            })
            .collect();
        let white = frame_white(&frame, n as u32, [0, 0, n as u32, 1]).unwrap();
        assert!(
            (scene_stops(white.unwrap()) - 2.91).abs() < 1e-3,
            "{white:?}"
        );
        let none = frame_white(&field([f32::NAN, 1.0, 1.0], 4), 4, [0, 0, 4, 1]).unwrap();
        assert_eq!(none, None);
    }

    #[test]
    fn a_blue_or_green_highlight_reads_as_bright_as_a_red_one() {
        // A frame whose highlights are sky blue: red sits at +1.2, blue at +2.4. Read on
        // red, the white would be +1.2 (floored to +1.5, contrast 1.65) and the sky would
        // land 1.5 stops past white; on the brightest channel the frame is clamped.
        for sky in [[at(1.2), at(1.8), at(2.4)], [at(1.2), at(2.4), at(1.6)]] {
            let mut frame = field([at(0.0); 3], 900);
            frame.extend(field(sky, 100));
            let white = frame_white(&frame, 1000, [0, 0, 1000, 1]).unwrap().unwrap();
            assert!((scene_stops(white) - 2.4).abs() < 1e-5, "{sky:?}");
            let w = place_roll_white(&[Some(scene_stops(white))]).unwrap();
            assert_eq!(w.roles, [FrameRole::Clamped]);
        }
    }

    #[test]
    fn a_frame_near_its_leader_keeps_its_white_warns_and_is_clamped() {
        // A frame whose top 10% sits 0.13 stop under its leader — inside the guard's
        // band, so the pool keeps none of it; the white still sees it.
        let leader = field([at(4.0); 3], 4);
        let highlight = at(4.0 - 0.13);
        let mut frame = field([at(1.0); 3], 900);
        frame.extend(field([highlight; 3], 100));
        let guard = leader_guard(&leader, 4, 1, 1.8).unwrap();
        let counts =
            pool_frame(&frame, 1000, [0, 0, 1000, 1], Some(&guard), &mut Vec::new()).unwrap();
        assert_eq!(counts.guarded, 100, "the highlight is inside the guard");
        let white = frame_white(&frame, 1000, [0, 0, 1000, 1]).unwrap().unwrap();
        assert_eq!(white, highlight);
        let d = leader_distance_stops(white, leader_peak(&leader, 4, 1).unwrap().unwrap());
        assert!((d - 0.13).abs() < 1e-4, "{d}");
        assert!(near_saturation(d));
        let w = place_roll_white(&[Some(scene_stops(white)), Some(1.7)]).unwrap();
        assert_eq!(w.roles[0], FrameRole::Clamped);
    }

    #[test]
    fn a_bright_frame_far_from_its_leader_does_not_warn() {
        let d = leader_distance_stops(at(2.7), at(4.5));
        assert!((d - 1.8).abs() < 1e-4, "{d}");
        assert!(!near_saturation(d));
    }
}
