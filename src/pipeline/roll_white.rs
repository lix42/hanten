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
//! **The roll's slope** (`nf-calibration/span-roll-slope`) spreads the roll's span, from
//! its dark end ([`roll_dark`]) to its white, over [`SPAN_LOOK_STOPS`], mid-grey pinned
//! and the roll's exposure unchanged, so the white renders where the slope and exposure
//! put it ([`span_slope`]). The recipe stores the two ends (`roll.white_stops`,
//! `roll.dark_stops`), not the slope, which is derived at render time and which
//! `look.contrast` multiplies.
//!
//! **The envelope hybrid** (`nf-calibration/envelope-hybrid-placement`, the default
//! placement, a preference) gives each frame its own slope and exposure inside the roll's
//! envelope ([`roll_envelope`], [`hybrid_placement`]), in place of the lifts below. It is
//! solved at render time from each frame's measured [`FrameStops`], so its knobs need no
//! re-measure.
//!
//! **The roll's white** (`nf-calibration/roll-white-rule`) is measured from the same
//! per-frame samples. The rule was chosen by review on nine rolls
//! (`nf-calibration/anchor-comparison`) when it placed the white at diffuse white, with a
//! black point in the chain; its values are provisional, since that sample held no
//! deliberately bad frames:
//!
//! - A frame's white is the [`WHITE_PERCENTILE`] of its pixels' **brightest channel**
//!   over its effective area, which keeps the specular headroom above white, in **scene
//!   stops** above mid-grey — on the decode's film RGB, before the working-space 3×3,
//!   **after the roll's colour correction** (its gains and its midtone line, mapped back
//!   through the 3×3: [`corrected_white`]). The rule's cap and floor were tuned on whites
//!   measured before it.
//! - The roll's white is the brightest frame white at or under [`WHITE_CAP_STOPS`],
//!   raised to at least [`WHITE_FLOOR_STOPS`]; if every frame is above the cap, it is the
//!   cap. A frame above the cap is **clamped**: it renders at the cap's span, not the
//!   roll's, and the report discloses it — an ordinary bright scene is clamped too, so it
//!   is not a warning.
//! - A frame whose white **as decoded** is within [`SATURATION_MARGIN_STOPS`] of the
//!   leader is near film saturation, and that warns. Without a leader there is no check.
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
//! frame darker than its roll stays dark, beyond a bounded lift. Each frame's level is the log-average of its
//! luma in ACEScg ([`frame_level`]); the roll's exposure brings the median frame level to
//! [`LEVEL_TARGET_STOPS`], within [`EXPOSURE_BOUND_EV`] ([`roll_exposure`]). A median over
//! frames, so one night scene cannot set it — the failure that retired per-frame auto
//! white balance. **The white is measured at exposure 0**: re-placing it after the
//! exposure was not better in review, and a small exposure could push one frame over the
//! cap and move the roll's slope by a third.
//!
//! **A frame's lift** (`nf-calibration/frame-level-trim`) is a small per-frame exposure on
//! top of the roll's, stored as a delta in `roll.frames` ([`small_lift`]). It is keyed on
//! where the frame's white renders after the roll's exposure: a low-key frame is lifted, a
//! bright one is not, and nothing is darkened. Not on the white alone, which re-does the
//! roll's exposure on a thin roll (evidence: `docs/progress/nf-calibration.md`,
//! `## frame-level-trim`).
//!
//! **A flat frame gets no lift** ([`FLAT_SPREAD_STOPS`]): one surface filling the frame has
//! no white of its own, and raising it only greys it.
//!
//! **A thin frame's lift** (`nf-calibration/thin-frame-lift`, on with an opt-out: taste, not
//! quality) is a bounded fine-tune, not a placement: a thin frame need not hold a white, so
//! nothing is solved to reach diffuse white. It raises the frame's white by about
//! [`THIN_LIFT_STOPS`] with the film base held about where its small lift renders it — a
//! steeper slope ([`thin_lift`], which says why "about") and the exposure that keeps the base
//! still — in place of the small lift. No statistic separates an underexposed frame from a
//! night scene; review preferred the lift on both.

use serde::{Deserialize, Serialize};

use crate::pipeline::colorimetry::pinned::ACESCG_LUMA;
use crate::pipeline::look::MID_GREY;
use crate::pipeline::midtone_neutral::MidtoneLine;
use crate::pipeline::roll_balance::RollBalance;
use crate::pipeline::white_balance::{
    green_anchored_gains, nearest_rank_index, percentile_levels, percentile_levels_of,
    sample_region,
};
use crate::pipeline::working_space::{acescg_to_film_rgb, film_rgb_to_acescg};
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

/// The white a render without a roll measurement is placed as if the roll had, in scene
/// stops above mid-grey: the `default` rendering's fallback slope
/// ([`crate::pipeline::look::DEFAULT_SLOPE`]). Chosen by review over +2.23 (the bundled
/// decode's contrast, above the cap) and the floor (`nf-calibration/no-roll-defaults`).
pub const FALLBACK_WHITE_STOPS: f32 = 1.75;

/// How close to the leader, in scene stops, a frame's white may come before the frame
/// warns as near film saturation. A placeholder, owned by
/// `nf-calibration/saturation-margin`: set from one frame judged overexposed. A leader
/// can sit below the film's shoulder, so a frame near it need not be saturated.
pub const SATURATION_MARGIN_STOPS: f32 = 0.5;

/// Where a roll's exposure puts its median frame level ([`frame_level`]), in scene stops
/// from mid-grey: mid-grey itself, where a light meter would. Chosen by review over −0.6
/// on a good and a poorly developed roll (`nf-calibration/level-target-zero`).
pub const LEVEL_TARGET_STOPS: f32 = 0.0;

/// The most a measured roll exposure moves, either way, in EV: a check for wrong inputs.
/// The eleven archive rolls measure +0.62 to +2.34.
pub const EXPOSURE_BOUND_EV: f32 = 3.0;

/// The most a frame's lift adds, in EV: the step review compared (a target 0.3 stop
/// brighter); nothing larger was tested.
pub const LIFT_BOUND_EV: f32 = 0.3;

/// A frame whose rendered white (its white plus the roll's exposure, in scene stops) is
/// at or under this gets the whole [`LIFT_BOUND_EV`].
pub const LIFT_FULL_STOPS: f32 = 0.9;

/// A frame whose rendered white is at or over this gets no lift; between the two the lift
/// falls linearly.
pub const LIFT_NONE_STOPS: f32 = 1.5;

/// A frame whose luma spans fewer stops than this from its p5 to its p95 is **flat** — one
/// surface filling the frame — and gets no lift of either kind: a lift lost on the two
/// such frames of ten rolls (0.59 and 0.67 stop), and won on none under 1.36.
pub const FLAT_SPREAD_STOPS: f32 = 1.0;

/// A thin-frame lift qualifies a frame whose rendered white (as [`LIFT_FULL_STOPS`]
/// measures it) is at or under this…
pub const THIN_WHITE_STOPS: f32 = LIFT_FULL_STOPS;

/// …and whose luma has at least this share within [`NEAR_BASE_STOPS`] of the film base:
/// its shadows sit on the base, which a normal low-key frame's do not. A night scene's do
/// too, and review preferred the lift there.
pub const THIN_BASE_SHARE: f32 = 0.3;

/// How close to the base, in scene stops, a pixel's luma counts toward
/// [`THIN_BASE_SHARE`].
pub const NEAR_BASE_STOPS: f32 = 1.0;

/// How far a thin-frame lift raises the frame's white, in rendered stops, with the film
/// base held about where the frame's small lift renders it ([`thin_lift`]). Chosen by
/// review over 0.5.
pub const THIN_LIFT_STOPS: f32 = 1.0;

/// The steepest slope a thin-frame lift may reach — grain rises with it. Review passed 2.4
/// on two thin frames.
pub const THIN_SLOPE_BOUND: f32 = 2.4;

/// The percentile of a frame's luma taken as its dark end ([`FrameTones::dark_stops`]).
/// From the poor-development spike (`docs/spike/poor-development.md`), whose 5 % is
/// untried.
pub const DARK_PERCENTILE: f32 = 0.01;

/// The percentile of its frames' dark ends taken as the roll's ([`roll_dark`]): low, so
/// the roll's darkest content sets it, but not its single darkest frame.
pub const ROLL_DARK_PERCENTILE: f32 = 0.1;

/// How many look stops the roll's span renders over ([`span_slope`]): the spike's white
/// target (+4.049, about L\* 91 on SDR) less its dark target (−5).
pub const SPAN_LOOK_STOPS: f32 = 9.049;

/// The steepest slope any placement sets: a guard against stretching a short span into
/// grain (`nf-calibration/short-span-protection`). `look.contrast` multiplies past it.
pub const MAX_SLOPE: f32 = 3.5;

/// The fewest look stops a hybrid frame's span renders over ([`hybrid_placement`]): R in
/// the poor-development spike. On a thin roll the envelope holds the dark end, so a higher
/// R brightens rather than deepens. Changing it moves default renders: bump
/// `PIPELINE_VERSION` by hand (the drift gate cannot see it).
pub const HYBRID_SPAN_STOPS: f32 = 8.0;

/// The share of its level's offset that a hybrid frame keeps: α in the spike. 1 keeps every
/// frame's level, 0 brings each to the level target — the roll's median unless the roll's
/// exposure was clamped or stated ([`hybrid_placement`]).
pub const HYBRID_LEVEL_KEEP: f32 = 0.6;

/// How far under the roll's exposure a frame's own placement may sit, in EV. With
/// [`FRAME_EXPOSURE_ABOVE_EV`], reviewed at ±1 over the spike's −1.5 / +0.5
/// (`nf-calibration/per-frame-placement`).
pub const FRAME_EXPOSURE_BELOW_EV: f32 = 1.0;

/// How far over the roll's exposure a frame's own placement may sit, in EV.
pub const FRAME_EXPOSURE_ABOVE_EV: f32 = 1.0;

/// Where the per-frame placement puts each frame's white, in look stops: about L\* 93 on
/// SDR, chosen by review over the span's L\* 91 (4.049). Provisional.
pub const FRAME_WHITE_LOOK_STOPS: f32 = 4.462;

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

/// The frame's sample over `region`: the pixels [`sample_white`] reads, in the same
/// positions as [`pool_frame`]'s.
pub fn frame_sample(rgb: &[f32], width: u32, region: [u32; 4]) -> Result<Vec<f32>> {
    sample_region(rgb, width, region, FRAME_SAMPLE_PIXELS)
}

/// A film-RGB sample ([`frame_sample`]) mapped to ACEScg, pixel for pixel as the decode
/// maps it.
pub fn acescg_sample(film: &[f32]) -> Vec<f32> {
    film.as_chunks::<3>()
        .0
        .iter()
        .flat_map(|px| film_rgb_to_acescg(*px))
        .collect()
}

/// A frame's white: the [`WHITE_PERCENTILE`] of its sample's usable pixels' brightest
/// channel ([`frame_sample`]), **guard or no guard**; `None` when no pixel is usable.
///
/// **The brightest channel, not one channel**, so a highlight reads bright whatever its
/// colour. The review measured red; on a blue- or green-lit highlight red under-reads —
/// on two Ektar rolls, by 0.5–2 stops on most frames — and the solved contrast then
/// pushes that highlight past white. On a neutral or warm highlight the two agree, so the
/// reviewed cap and floor keep their meaning (the roll's white moved ≤ 0.2 stop on nine
/// rolls). A saturated coloured highlight now sets the white; highlight desaturation
/// leaves such a pixel's colour alone (`pipeline::look`).
///
/// The sample is the decode's **film RGB**, before the working-space 3×3 — the domain the
/// rule was reviewed in. The ACEScg values after the 3×3 mix the channels, and with them
/// the roll's uncorrected cast: measured there, red moved frames −0.5 to +0.6 stop, by
/// stock, off the reviewed values.
///
/// `measure-roll` measures it twice from one sample: as decoded, for the saturation check
/// against the leader, and after the roll's colour correction ([`corrected_white`]), for
/// the rule.
pub fn sample_white(sample: &[f32]) -> Option<f32> {
    let peaks = usable_peaks(sample);
    (!peaks.is_empty()).then(|| nearest_rank_of(peaks, WHITE_PERCENTILE))
}

/// `sample_white` over `region` of a whole frame, as the tests state one.
#[cfg(test)]
fn frame_white(rgb: &[f32], width: u32, region: [u32; 4]) -> Result<Option<f32>> {
    Ok(sample_white(&frame_sample(rgb, width, region)?))
}

/// A frame's white **after the roll's colour correction**: each film-RGB pixel of
/// `sample` mapped to ACEScg, corrected by the midtone `line` (if any) and the roll's
/// `balance`, and mapped back, so the rule keeps the domain it was reviewed in.
///
/// Measured before correction, any colour correction moved every frame's white, hence
/// the roll's slope (`docs/spike/poor-development.md`, "Measurement order"). No loop:
/// the look's slope is channel-equal and pivoted at mid-grey, so a neutral white stays
/// neutral.
pub fn corrected_white(
    sample: &[f32],
    balance: &RollBalance,
    line: Option<&MidtoneLine>,
) -> Option<f32> {
    let corrected: Vec<f32> = sample
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|px| {
            acescg_to_film_rgb(MidtoneLine::balanced(
                line,
                film_rgb_to_acescg(*px),
                balance,
            ))
        })
        .collect();
    sample_white(&corrected)
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
        .map(luma)
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
    let level_stops = median(&mut known);
    let wanted = LEVEL_TARGET_STOPS - level_stops;
    let ev = wanted.clamp(-EXPOSURE_BOUND_EV, EXPOSURE_BOUND_EV);
    Ok(RollExposure {
        ev,
        level_stops,
        bounded: ev != wanted,
    })
}

/// The roll's dark end, in scene stops: the [`ROLL_DARK_PERCENTILE`] of its frames' dark
/// ends ([`FrameTones::dark_stops`]; `None` for a frame with no usable pixel), so one
/// night frame does not set it. `None` when no frame has one.
pub fn roll_dark(darks: &[Option<f32>]) -> Option<f32> {
    let known: Vec<f32> = darks.iter().flatten().copied().collect();
    (!known.is_empty()).then(|| nearest_rank_of(known, ROLL_DARK_PERCENTILE))
}

/// The slope that renders a span from `dark_stops` to `white_stops` (scene stops) over
/// [`SPAN_LOOK_STOPS`], held within [`slope_for`] of [`WHITE_CAP_STOPS`] (1.237)
/// and [`MAX_SLOPE`]. A span that is not positive takes the steepest.
pub fn span_slope(white_stops: f32, dark_stops: f32) -> f32 {
    let span = white_stops - dark_stops;
    let k = if span > 0.0 {
        SPAN_LOOK_STOPS / span
    } else {
        MAX_SLOPE
    };
    k.clamp(slope_for(WHITE_CAP_STOPS), MAX_SLOPE)
}

/// A frame's own ends and level, in scene stops: what the hybrid and `frame` place it from
/// (`roll.frame_tones`, a `roll.frames` entry's `tones`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameStops {
    /// Its white ([`sample_white`], after the roll's colour correction), unclamped.
    pub white_stops: f32,
    /// Its level ([`frame_level`]).
    pub level_stops: f32,
    /// Its dark end ([`FrameTones::dark_stops`]).
    pub dark_stops: f32,
}

/// The roll's envelope, `[darkest, brightest]` in scene stops: its frames' lowest dark end
/// and highest white, unclamped (`roll.envelope_stops`). `None` without a frame.
pub fn roll_envelope(frames: &[FrameStops]) -> Option<[f32; 2]> {
    let darkest = frames.iter().map(|f| f.dark_stops).min_by(f32::total_cmp)?;
    let brightest = frames
        .iter()
        .map(|f| f.white_stops)
        .max_by(f32::total_cmp)?;
    Some([darkest, brightest])
}

/// What set a frame's own slope ([`hybrid_placement`], [`per_frame_placement`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlopeBound {
    /// The roll's slope: the frame's span already renders over the minimum.
    Roll,
    /// The frame's span, spread over the placement's look stops (the hybrid's minimum).
    Span,
    /// The envelope: steeper would carry the frame past the roll's ends.
    Envelope,
    /// [`MAX_SLOPE`].
    Max,
    /// The exposure limit: held there, a steeper slope would carry an end past the
    /// envelope ([`hybrid_placement`]).
    Limit,
    /// The cap's slope, the flattest any placement sets.
    Floor,
}

/// What set a frame's own exposure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExposureBound {
    /// The placement's own rule: the share of its level offset it keeps, or its white.
    None,
    /// The envelope: the frame's white or dark end would cross the roll's.
    Envelope,
    /// The frame's span is wider than the envelope: centred in it.
    Centred,
    /// [`FRAME_EXPOSURE_BELOW_EV`] or [`FRAME_EXPOSURE_ABOVE_EV`].
    Limit,
}

/// A frame's own placement: its slope and exposure.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct FramePlacement {
    /// The look's base slope, in place of the roll's.
    pub slope: f32,
    /// In EV, a delta on the roll's exposure, in place of the lifts.
    pub exposure: f32,
    pub slope_bound: SlopeBound,
    pub exposure_bound: ExposureBound,
}

/// Place one frame inside the roll's envelope (`nf-calibration/envelope-hybrid-placement`;
/// the reviewed definition is `docs/spike/poor-development.md`, "As reviewed"). The
/// frame's slope spreads its span over at least `span_stops` look stops, within the
/// roll's per-roll render of the envelope (`roll_slope · (envelope + roll_ev)`), between
/// the cap's slope and [`MAX_SLOPE`]; its exposure keeps `level_keep` of its level's offset
/// from [`LEVEL_TARGET_STOPS`], within the envelope, then within the limits around the roll's. Where
/// the limit holds it, the slope flattens until both ends stay inside the envelope
/// (`nf-calibration/hybrid-slope-bounds`), never under the floor.
///
/// `roll_slope` is the frame's per-roll slope — the roll's span slope, or a clamped
/// frame's — so a clamped frame's envelope is the one per-roll renders it in.
pub fn hybrid_placement(
    frame: FrameStops,
    roll_slope: f32,
    roll_ev: f32,
    [darkest, brightest]: [f32; 2],
    span_stops: f32,
    level_keep: f32,
) -> FramePlacement {
    let FrameStops {
        white_stops: w,
        level_stops: level,
        dark_stops: d,
    } = frame;
    let span = w - d;
    let per_span = |stops: f32| if span > 0.0 { stops / span } else { MAX_SLOPE };
    let (top, bottom) = (
        roll_slope * (brightest + roll_ev),
        roll_slope * (darkest + roll_ev),
    );
    let wanted = per_span(span_stops);
    let fit = per_span(top - bottom);
    let floor = roll_slope.min(slope_for(WHITE_CAP_STOPS));
    let candidates = [(roll_slope, SlopeBound::Roll), (wanted, SlopeBound::Span)];
    let (mut slope, mut slope_bound) = candidates
        .into_iter()
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .expect("two candidates");
    for (limit, bound) in [(fit, SlopeBound::Envelope), (MAX_SLOPE, SlopeBound::Max)] {
        if limit < slope {
            (slope, slope_bound) = (limit, bound);
        }
    }
    if slope < floor {
        (slope, slope_bound) = (floor, SlopeBound::Floor);
    }
    // Exposure in total EV, as the roll's is.
    // The offset is from the level target, which is the roll's level unless its exposure
    // was clamped (`EXPOSURE_BOUND_EV`) or stated: then frames pull towards the target.
    let kept = roll_ev - (1.0 - level_keep) * (level + roll_ev - LEVEL_TARGET_STOPS);
    let (lo, hi) = (bottom / slope - d, top / slope - w);
    let (mut e, mut exposure_bound) = if lo > hi {
        ((lo + hi) / 2.0, ExposureBound::Centred)
    } else if kept < lo || kept > hi {
        (kept.clamp(lo, hi), ExposureBound::Envelope)
    } else {
        (kept, ExposureBound::None)
    };
    (e, exposure_bound) = within_frame_limits(e, roll_ev, exposure_bound);
    // Held at the limit, an end may cross the envelope: flatten until both are inside.
    // Flattening pulls an end towards mid-grey, so it helps only an end on the same side
    // of mid-grey as the envelope's.
    if exposure_bound == ExposureBound::Limit {
        let mut k = slope;
        if d + e < 0.0 && bottom < 0.0 {
            k = k.min(bottom / (d + e));
        }
        if w + e > 0.0 && top > 0.0 {
            k = k.min(top / (w + e));
        }
        if k < slope {
            (slope, slope_bound) = if k < floor {
                (floor, SlopeBound::Floor)
            } else {
                (k, SlopeBound::Limit)
            };
        }
    }
    FramePlacement {
        slope,
        exposure: e - roll_ev,
        slope_bound,
        exposure_bound,
    }
}

/// Place one frame on its own (`nf-calibration/per-frame-placement`), the roll's envelope
/// playing no part: its slope spreads its span over [`SPAN_LOOK_STOPS`], between the cap's
/// slope and [`MAX_SLOPE`] (the roll's span slope, from the frame's own ends); its exposure
/// puts its white at [`FRAME_WHITE_LOOK_STOPS`], within the limits around the roll's. A
/// per-frame slope needs a per-frame anchor: pivoted at the roll's mid-grey it pushed
/// low-key frames' highlights past white (`docs/spike/poor-development.md`). The review
/// preferred the white as anchor over the frame's own midtone.
pub fn per_frame_placement(frame: FrameStops, roll_ev: f32) -> FramePlacement {
    let FrameStops {
        white_stops: w,
        dark_stops: d,
        ..
    } = frame;
    let slope = span_slope(w, d);
    let slope_bound = if slope == MAX_SLOPE {
        SlopeBound::Max
    } else if slope == slope_for(WHITE_CAP_STOPS) {
        SlopeBound::Floor
    } else {
        SlopeBound::Span
    };
    // Exposure in total EV, as the roll's is.
    let (e, exposure_bound) = within_frame_limits(
        FRAME_WHITE_LOOK_STOPS / slope - w,
        roll_ev,
        ExposureBound::None,
    );
    FramePlacement {
        slope,
        exposure: e - roll_ev,
        slope_bound,
        exposure_bound,
    }
}

/// A frame's own exposure `e` (total EV) held within [`FRAME_EXPOSURE_BELOW_EV`] under and
/// [`FRAME_EXPOSURE_ABOVE_EV`] over the roll's; `bound` stands unless the limit moves it.
fn within_frame_limits(e: f32, roll_ev: f32, bound: ExposureBound) -> (f32, ExposureBound) {
    let (below, above) = (
        roll_ev - FRAME_EXPOSURE_BELOW_EV,
        roll_ev + FRAME_EXPOSURE_ABOVE_EV,
    );
    if e < below || e > above {
        (e.clamp(below, above), ExposureBound::Limit)
    } else {
        (e, bound)
    }
}

/// The median of a non-empty set, sorting it; the mean of the middle two when even.
pub fn median(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

/// A frame's lift in EV, `0..=LIFT_BOUND_EV`: from its white ([`sample_white`], scene
/// stops) and the roll's exposure as applied, so the key is where the white renders.
pub fn small_lift(white_stops: f32, roll_ev: f32) -> f32 {
    let rendered = white_stops + roll_ev;
    let t = (LIFT_NONE_STOPS - rendered) / (LIFT_NONE_STOPS - LIFT_FULL_STOPS);
    LIFT_BOUND_EV * t.clamp(0.0, 1.0)
}

/// A frame's tonal shape over `region`, from its luma in scene stops: what decides whether
/// a lift may touch it ([`FLAT_SPREAD_STOPS`], [`THIN_BASE_SHARE`]), and its dark end.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct FrameTones {
    /// Stops from the luma's p5 to its p95.
    pub spread_stops: f32,
    /// The luma's [`DARK_PERCENTILE`]: the frame's dark end, which [`roll_dark`] pools.
    /// Before the roll's gains, as the exposure's level is (the white is after them): the
    /// gains move a dark pixel's luma by hundredths of a stop.
    pub dark_stops: f32,
    /// The share of pixels whose luma is within [`NEAR_BASE_STOPS`] of the base.
    pub base_share: f32,
}

impl FrameTones {
    pub fn flat(&self) -> bool {
        self.spread_stops < FLAT_SPREAD_STOPS
    }
}

/// [`FrameTones`] over `region` of the decode's **linear ACEScg**, the pixels
/// [`frame_level`] counts, against `base_stops` ([`base_stops`]); `None` when none counts.
pub fn frame_tones(
    rgb: &[f32],
    width: u32,
    region: [u32; 4],
    base_stops: f32,
) -> Result<Option<FrameTones>> {
    let sample = sample_region(rgb, width, region, FRAME_SAMPLE_PIXELS)?;
    let stops: Vec<f32> = sample
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|px| px.iter().all(|v| v.is_finite()))
        .map(luma)
        .filter(|y| *y > 0.0)
        .map(scene_stops)
        .collect();
    if stops.is_empty() {
        return Ok(None);
    }
    let near = stops
        .iter()
        .filter(|s| **s < base_stops + NEAR_BASE_STOPS)
        .count();
    let base_share = near as f32 / stops.len() as f32;
    // Every percentile from nested selections: each lies at or below the next one's place.
    let mut stops = stops;
    let (dark, lo, hi) = (
        nearest_rank_index(stops.len(), DARK_PERCENTILE),
        nearest_rank_index(stops.len(), 0.05),
        nearest_rank_index(stops.len(), 0.95),
    );
    let (below, p95, _) = stops.select_nth_unstable_by(hi, f32::total_cmp);
    let p95 = *p95;
    let (below, p5) = if lo < hi {
        let (below, p5, _) = below.select_nth_unstable_by(lo, f32::total_cmp);
        (below, *p5)
    } else {
        (below, p95)
    };
    let p1 = if dark < lo.min(hi) {
        *below.select_nth_unstable_by(dark, f32::total_cmp).1
    } else {
        p5
    };
    Ok(Some(FrameTones {
        spread_stops: p95 - p5,
        dark_stops: p1,
        base_share,
    }))
}

fn luma(px: &[f32; 3]) -> f32 {
    (0..3).map(|c| ACESCG_LUMA[c] * px[c]).sum()
}

/// The decoded film base (one ACEScg pixel, `algo::fixed::decode_film_base` mapped) as a
/// luma in scene stops: where every frame's shadows bottom out.
pub fn base_stops(px: [f32; 3]) -> f32 {
    scene_stops(luma(&px))
}

/// Whether a frame qualifies for a thin-frame lift: its white renders low after the roll's
/// exposure and its shadows sit on the base. A flat frame never does.
pub fn thin(white_stops: f32, roll_ev: f32, tones: &FrameTones) -> bool {
    !tones.flat()
        && white_stops + roll_ev <= THIN_WHITE_STOPS
        && tones.base_share >= THIN_BASE_SHARE
}

/// A thin frame's own slope and exposure.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ThinLift {
    /// The frame's slope: `roll.thin_slope`.
    pub slope: f32,
    /// Its exposure in EV, a delta on the roll's: `roll.thin_exposure`, in place of the
    /// frame's [`small_lift`].
    pub exposure: f32,
    /// How far its white rises, in rendered stops, by [`thin_lift`]'s measure:
    /// [`THIN_LIFT_STOPS`] unless bounded.
    pub lift_stops: f32,
    /// Whether [`THIN_SLOPE_BOUND`] held the lift under [`THIN_LIFT_STOPS`].
    pub bounded: bool,
}

/// Raise a thin frame's white about [`THIN_LIFT_STOPS`] with the film base held about where
/// its small lift renders it: from slope `k0` and total exposure `e0` (the roll's plus the frame's
/// [`small_lift`]) to slope `k0 + Δ/(white − base)`, capped at [`THIN_SLOPE_BOUND`], and
/// the exposure that keeps `k·(base + e)` where it was. `None` when no lift is possible:
/// the white is not above the base, `k0` is already at the bound, or the exposure would
/// fall under `e0` (a base above mid-grey), darkening the mid-tones the small lift raised.
///
/// Approximate: the white (film RGB, brightest channel) and the base (ACEScg luma, before
/// the roll's gains) are not the luminance the render's slope applies to, so the base
/// moves and the white rises by about, not exactly, these amounts.
pub fn thin_lift(
    white_stops: f32,
    base_stops: f32,
    roll_ev: f32,
    k0: f32,
    e0: f32,
) -> Option<ThinLift> {
    let range = white_stops - base_stops;
    if range <= 0.0 || k0 >= THIN_SLOPE_BOUND {
        return None;
    }
    let wanted = k0 + THIN_LIFT_STOPS / range;
    let slope = wanted.min(THIN_SLOPE_BOUND);
    let e = k0 * (base_stops + e0) / slope - base_stops;
    if e < e0 {
        return None;
    }
    Some(ThinLift {
        slope,
        exposure: e - roll_ev,
        lift_stops: (slope - k0) * range,
        bounded: slope < wanted,
    })
}

/// The median of the leader's brightest channel over its centre half, in the same
/// film-RGB measure as [`sample_white`] — what a frame's saturation distance is measured
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
/// already include the decode's linearization, so it does not enter here. The fallback
/// without a roll measurement, and the cap's, the flattest [`span_slope`].
pub const fn slope_for(white_stops: f32) -> f32 {
    WHITE_OVER_MID_STOPS / white_stops
}

/// `log2(DIFFUSE_WHITE / MID_GREY)`, written out so [`slope_for`] needs no libm and can
/// define a constant (`tests::white_over_mid_is_the_log_of_the_ratio`).
const WHITE_OVER_MID_STOPS: f32 = 2.473_931;

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
    /// Above the cap: rendered at the cap's span to the roll's dark end, not the roll's.
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

/// Where the midtone line fades to zero: the scene stop of the luma of the roll's pooled
/// per-channel [`PERCENTILE`] after `balance`, over every frame's ACEScg sample
/// ([`acescg_sample`]).
///
/// **No leader guard**, as reviewed: the guard drops a poorly developed roll's real
/// highlights near the leader, and on 09-29 Ektar put this 0.23 stop under the reviewed
/// value. Not the gains' own p99, which is guarded.
pub fn pooled_white_stops(samples: &[Vec<f32>], balance: &RollBalance) -> f32 {
    let usable = samples
        .iter()
        .flat_map(|s| s.as_chunks::<3>().0.iter())
        .filter(|px| !unusable(px));
    let cap = samples.iter().map(|s| s.len() / 3).sum();
    let p = percentile_levels_of(usable, cap, PERCENTILE);
    let luma: f32 = match balance {
        RollBalance::Gain(gains) => (0..3).map(|c| ACESCG_LUMA[c] * p[c] * gains[c]).sum(),
        RollBalance::Tilt(t) => luma(&t.apply(p)),
    };
    scene_stops(luma)
}

/// Green's level at the white [`roll_gains`] equalizes, in scene stops: where a tilt
/// reaches the full gains (`pipeline::roll_balance`). Same pool, same percentile.
pub fn roll_white_balance_stops(pool: &[f32]) -> f32 {
    scene_stops(percentile_levels(pool, PERCENTILE)[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::fixed::DIFFUSE_WHITE;

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
        let bright = roll_exposure(&[Some(4.0)]).unwrap();
        assert_eq!((bright.ev, bright.bounded), (-EXPOSURE_BOUND_EV, true));
        assert!(roll_exposure(&[None, None]).is_err());
    }

    #[test]
    fn a_small_lift_follows_its_rendered_white_and_never_darkens() {
        // Keyed on white + roll exposure: the same white lifts less on a lifted roll.
        assert_eq!(small_lift(-1.5, 0.0), LIFT_BOUND_EV);
        assert_eq!(small_lift(LIFT_FULL_STOPS - 1.0, 1.0), LIFT_BOUND_EV);
        assert_eq!(small_lift(LIFT_NONE_STOPS, 0.0), 0.0);
        assert_eq!(small_lift(4.0, 1.5), 0.0);
        let mid = small_lift((LIFT_FULL_STOPS + LIFT_NONE_STOPS) / 2.0 - 0.5, 0.5);
        assert!((mid - LIFT_BOUND_EV / 2.0).abs() < 1e-6, "{mid}");
        // A night frame (white far under) is held at the bound.
        assert_eq!(small_lift(-8.0, 2.0), LIFT_BOUND_EV);
    }

    #[test]
    fn a_thin_lift_raises_the_white_and_holds_the_base() {
        // Frame 2005 of the 2026-09-28 roll, as review round 1 rendered it.
        let (white, base, roll_ev) = (-1.2267, -3.707, 1.3933);
        let (k0, e0) = (slope_for(WHITE_FLOOR_STOPS), roll_ev + LIFT_BOUND_EV);
        let t = thin_lift(white, base, roll_ev, k0, e0).unwrap();
        assert!((t.slope - 2.052).abs() < 1e-3, "{t:?}");
        assert!((t.exposure - 0.696).abs() < 1e-3, "{t:?}");
        assert!((t.lift_stops - THIN_LIFT_STOPS).abs() < 1e-5, "{t:?}");
        assert!(!t.bounded);
        let rendered = |k: f32, e: f32, s: f32| k * (s + e);
        let e = roll_ev + t.exposure;
        assert!((rendered(t.slope, e, base) - rendered(k0, e0, base)).abs() < 1e-4);
        assert!(
            (rendered(t.slope, e, white) - rendered(k0, e0, white) - THIN_LIFT_STOPS).abs() < 1e-4
        );
        // A white close to the base asks for a slope past the bound: capped, and said so.
        let t = thin_lift(base + 0.5, base, roll_ev, k0, e0).unwrap();
        assert_eq!(t.slope, THIN_SLOPE_BOUND);
        assert!(t.bounded && t.lift_stops < THIN_LIFT_STOPS, "{t:?}");
        // Nothing to spread, or no room under the bound.
        assert_eq!(thin_lift(base, base, roll_ev, k0, e0), None);
        assert_eq!(thin_lift(white, base, roll_ev, THIN_SLOPE_BOUND, e0), None);
        // A base above mid-grey would solve an exposure under the small lift's: none.
        let (white, base) = (1.5, 0.5);
        let k0 = slope_for(white);
        assert!(k0 * (base + e0) > 0.0 && k0 < THIN_SLOPE_BOUND);
        assert_eq!(thin_lift(white, base, roll_ev, k0, e0), None);
    }

    #[test]
    fn a_frames_tones_tell_flat_and_base_bound_frames() {
        let base = -3.0;
        // Twenty levels a fifth of a stop apart: nearest-rank p5 to p95 spans 17 steps,
        // and the five within a stop of the base are a quarter.
        let rgb: Vec<f32> = (0..20)
            .flat_map(|i| {
                let y = MID_GREY * (base + 0.1 + 0.2 * i as f32).exp2();
                [y; 3]
            })
            .collect();
        let t = frame_tones(&rgb, 20, [0, 0, 20, 1], base).unwrap().unwrap();
        assert!((t.spread_stops - 3.4).abs() < 1e-4, "{t:?}");
        assert!((t.base_share - 0.25).abs() < 1e-6, "{t:?}");
        assert!((t.dark_stops - (base + 0.1)).abs() < 1e-4, "{t:?}");
        assert!(!t.flat());
        // One surface: under a stop from p5 to p95.
        let flat: Vec<f32> = (0..20)
            .flat_map(|i| [MID_GREY * (0.03 * i as f32).exp2(); 3])
            .collect();
        assert!(
            frame_tones(&flat, 20, [0, 0, 20, 1], base)
                .unwrap()
                .unwrap()
                .flat()
        );
        assert_eq!(
            frame_tones(&field([0.0; 3], 4), 4, [0, 0, 4, 1], base).unwrap(),
            None
        );
    }

    #[test]
    fn a_thin_frame_is_dark_with_its_shadows_on_the_base_and_not_flat() {
        let tones = |spread_stops, base_share| FrameTones {
            spread_stops,
            dark_stops: -3.0,
            base_share,
        };
        assert!(thin(-1.2, 1.4, &tones(1.9, 0.64)));
        // A normal low-key frame: as dark, but its shadows sit above the base.
        assert!(!thin(-0.55, 0.65, &tones(1.4, 0.02)));
        // Bright enough after the roll's exposure.
        assert!(!thin(0.5, 1.4, &tones(2.0, 0.6)));
        // Flat wins over everything.
        assert!(!thin(-1.2, 1.4, &tones(FLAT_SPREAD_STOPS - 0.01, 0.9)));
    }

    #[test]
    fn a_frames_dark_end_is_its_lumas_low_percentile() {
        // 200 levels a hundredth of a stop apart: nearest-rank p1 is the third.
        let rgb: Vec<f32> = (0..200)
            .flat_map(|i| [MID_GREY * (-4.0 + 0.01 * i as f32).exp2(); 3])
            .collect();
        let t = frame_tones(&rgb, 200, [0, 0, 200, 1], -5.0)
            .unwrap()
            .unwrap();
        assert!((t.dark_stops - -3.98).abs() < 1e-4, "{t:?}");
    }

    #[test]
    fn the_rolls_dark_end_is_low_but_not_its_darkest_frame() {
        let darks: Vec<Option<f32>> = (0..11).map(|i| Some(-6.0 + i as f32 * 0.1)).collect();
        // Nearest-rank p10 of eleven is the second darkest.
        assert_eq!(roll_dark(&darks), darks[1]);
        assert_eq!(roll_dark(&[None, Some(-4.0)]), Some(-4.0));
        assert_eq!(roll_dark(&[None]), None);
    }

    #[test]
    fn the_span_slope_spreads_the_span_over_its_look_stops_within_the_limits() {
        assert!((span_slope(1.75, -3.75) - SPAN_LOOK_STOPS / 5.5).abs() < 1e-6);
        // A very long span flattens no further than 1.237, a short one steepens
        // no further than the maximum; an empty span takes the steepest.
        assert_eq!(span_slope(2.0, -20.0), slope_for(WHITE_CAP_STOPS));
        assert_eq!(span_slope(1.0, -0.5), MAX_SLOPE);
        assert_eq!(span_slope(1.0, 1.0), MAX_SLOPE);
    }

    fn stops(white_stops: f32, level_stops: f32, dark_stops: f32) -> FrameStops {
        FrameStops {
            white_stops,
            level_stops,
            dark_stops,
        }
    }

    /// The default hybrid at the roll slope `k`, exposure `e0` and envelope `env`.
    fn hybrid(frame: FrameStops, k: f32, e0: f32, env: [f32; 2]) -> FramePlacement {
        hybrid_placement(frame, k, e0, env, HYBRID_SPAN_STOPS, HYBRID_LEVEL_KEEP)
    }

    #[test]
    fn the_envelope_is_the_lowest_dark_end_and_the_highest_white() {
        let frames = [stops(1.2, 0.0, -4.0), stops(2.6, 0.5, -3.0)];
        assert_eq!(roll_envelope(&frames), Some([-4.0, 2.6]));
        assert_eq!(roll_envelope(&[]), None);
    }

    #[test]
    fn a_hybrid_frame_stays_inside_the_envelope_the_roll_renders() {
        let (k, e0, env) = (1.65, 1.0, [-4.5, 1.9]);
        let (top, bottom) = (k * (env[1] + e0), k * (env[0] + e0));
        // Frames of every span and level the envelope holds, night frames the limit holds
        // among them.
        let mut held = 0;
        for white in [-2.5f32, -1.0, 0.3, 0.8, 1.4, 1.9] {
            for dark in [-4.5f32, -3.5, -2.0] {
                for level in [-4.0f32, -2.0, -1.0, 0.0] {
                    let p = hybrid(stops(white, level, dark), k, e0, env);
                    let e = e0 + p.exposure;
                    held += usize::from(p.exposure_bound == ExposureBound::Limit);
                    assert!(p.slope * (white + e) <= top + 1e-4, "{white} {dark}: {p:?}");
                    assert!(
                        p.slope * (dark + e) >= bottom - 1e-4,
                        "{white} {dark}: {p:?}"
                    );
                    assert!((slope_for(WHITE_CAP_STOPS)..=MAX_SLOPE).contains(&p.slope));
                    assert!(
                        p.slope >= k
                            || matches!(p.slope_bound, SlopeBound::Envelope | SlopeBound::Limit)
                    );
                }
            }
        }
        assert!(held > 0, "the grid reaches the exposure limit");
    }

    #[test]
    fn a_short_span_steepens_toward_the_minimum_and_a_wide_one_keeps_the_roll_slope() {
        let (k, e0, env) = (1.65, 1.0, [-6.0, 2.0]);
        // 4 stops: 8 / 4 = 2, inside the envelope's 1.65 · 8 / 4 = 3.3.
        let p = hybrid(stops(1.0, -1.0, -3.0), k, e0, env);
        assert_eq!(p.slope_bound, SlopeBound::Span);
        // A literal, so a new default R fails here and asks for a `PIPELINE_VERSION` bump.
        assert!((p.slope - 2.0).abs() < 1e-6, "{p:?}");
        // 7 stops already render over 7 · 1.65 > 8.
        let p = hybrid(stops(1.5, -1.0, -5.5), k, e0, env);
        assert_eq!((p.slope, p.slope_bound), (k, SlopeBound::Roll));
        // 1.5 stops would want 5.3: the envelope allows 8.8, the maximum 3.5.
        let p = hybrid(stops(0.5, 0.0, -1.0), k, e0, env);
        assert_eq!((p.slope, p.slope_bound), (MAX_SLOPE, SlopeBound::Max));
        // A narrow envelope: 1.65 · 2 / 2 = 1.65 at most, from a wanted 4.
        let p = hybrid(stops(1.0, 0.0, -1.0), k, e0, [-1.0, 1.0]);
        assert_eq!(p.slope_bound, SlopeBound::Envelope);
        assert!((p.slope - 1.65).abs() < 1e-5, "{p:?}");
        // A frame wider than its envelope flattens no further than the cap's slope.
        let p = hybrid(stops(2.0, 0.0, -8.0), k, e0, [-4.0, 1.0]);
        assert_eq!(
            (p.slope, p.slope_bound),
            (slope_for(WHITE_CAP_STOPS), SlopeBound::Floor)
        );
        // No span: the steepest the envelope allows.
        let p = hybrid(stops(0.5, 0.5, 0.5), k, e0, env);
        assert_eq!(p.slope, MAX_SLOPE);
    }

    #[test]
    fn a_hybrid_frame_keeps_its_share_of_the_level_offset_within_the_limits() {
        let (k, e0, env) = (1.65, 1.0, [-8.0, 3.0]);
        // At the roll's target already: the roll's exposure.
        let p = hybrid(stops(1.0, -e0, -3.0), k, e0, env);
        assert_eq!(p.exposure_bound, ExposureBound::None);
        assert!(p.exposure.abs() < 1e-6, "{p:?}");
        // A stop under it after the roll's exposure keeps 0.6 of that stop: lifted 0.4.
        let p = hybrid(stops(1.0, -e0 - 1.0, -3.0), k, e0, env);
        assert!((p.exposure - 0.4).abs() < 1e-6, "{p:?}");
        // Five stops over: darkened 2, held at the limit.
        let p = hybrid(stops(1.0, -e0 + 5.0, -3.0), k, e0, env);
        assert_eq!(p.exposure_bound, ExposureBound::Limit);
        assert!((p.exposure + FRAME_EXPOSURE_BELOW_EV).abs() < 1e-6, "{p:?}");
        // Level 1 keeps every frame where the roll's exposure puts it.
        let p = hybrid_placement(stops(1.0, 2.0, -3.0), k, e0, env, 7.0, 1.0);
        assert!(p.exposure.abs() < 1e-6, "{p:?}");
    }

    #[test]
    fn the_envelope_bounds_a_hybrid_frames_exposure() {
        let (k, e0) = (1.65, 1.0);
        // The brightest frame lifted would carry its white past the roll's: held at it.
        let env = [-5.0, 1.5];
        let p = hybrid(stops(1.5, -3.0, -1.5), k, e0, env);
        assert_eq!(p.exposure_bound, ExposureBound::Envelope);
        assert!(
            (p.slope * (1.5 + e0 + p.exposure) - k * (env[1] + e0)).abs() < 1e-4,
            "{p:?}"
        );
        // Wider than the envelope at the floor: centred in it.
        let p = hybrid(
            stops(1.5, 0.0, -5.0),
            slope_for(WHITE_CAP_STOPS),
            e0,
            [-4.0, 1.0],
        );
        assert_eq!(p.exposure_bound, ExposureBound::Centred);
    }

    #[test]
    fn a_frame_placed_on_its_own_spreads_its_own_span_whatever_the_roll() {
        let e0 = 1.0;
        // 9.049 / 4 stops, the roll's span slope from the frame's own ends.
        let p = per_frame_placement(stops(1.0, -1.0, -3.0), e0);
        assert_eq!(
            (p.slope, p.slope_bound),
            (span_slope(1.0, -3.0), SlopeBound::Span)
        );
        // Short: the maximum. Wide: the cap's slope. None: the maximum.
        let p = per_frame_placement(stops(1.0, 0.0, -1.0), e0);
        assert_eq!((p.slope, p.slope_bound), (MAX_SLOPE, SlopeBound::Max));
        let p = per_frame_placement(stops(2.0, 0.0, -8.0), e0);
        assert_eq!(
            (p.slope, p.slope_bound),
            (slope_for(WHITE_CAP_STOPS), SlopeBound::Floor)
        );
        let p = per_frame_placement(stops(0.5, 0.5, 0.5), e0);
        assert_eq!((p.slope, p.slope_bound), (MAX_SLOPE, SlopeBound::Max));
    }

    #[test]
    fn a_hybrid_frame_held_at_the_limit_flattens_to_stay_inside_the_envelope() {
        let (k, e0, env) = (1.6, 1.0, [-4.5, 2.4]);
        // A short night frame: its slope wants 2.8, and keeping its dark end inside the
        // envelope then wants 3 EV of lift, which the +1 EV limit refuses.
        let frame = stops(-2.5, -4.0, -5.0);
        let p = hybrid_placement(frame, k, e0, env, 7.0, 0.6);
        assert_eq!(p.exposure_bound, ExposureBound::Limit);
        assert_eq!(p.exposure, FRAME_EXPOSURE_ABOVE_EV);
        assert_eq!(p.slope_bound, SlopeBound::Limit);
        let dark = p.slope * (frame.dark_stops + e0 + p.exposure);
        assert!((dark - k * (env[0] + e0)).abs() < 1e-5, "{p:?}");
        // An envelope whose dark end renders above mid-grey (a stated roll exposure): a dark
        // end inside it is left alone.
        let (k, e0) = (1.7, 5.0);
        let p = hybrid_placement(stops(0.9, -1.4, -3.2), k, e0, [-4.5, 2.2], 7.0, 0.6);
        assert_eq!(p.exposure_bound, ExposureBound::Limit);
        assert_eq!(p.slope_bound, SlopeBound::Span);
        let (k, e0) = (1.6, 1.0);
        // Not under the floor, whatever the envelope asks.
        let p = hybrid_placement(stops(-2.5, -4.0, -9.0), k, e0, env, 7.0, 0.6);
        assert_eq!(
            (p.slope, p.slope_bound),
            (slope_for(WHITE_CAP_STOPS), SlopeBound::Floor)
        );
    }

    #[test]
    fn a_frame_placed_on_its_own_holds_its_white_within_the_limits() {
        let e0 = 1.0;
        // The frame's white renders at the target, whatever its level.
        let frame = stops(0.5, -1.0, -3.0);
        let p = per_frame_placement(frame, e0);
        assert_eq!(p.exposure_bound, ExposureBound::None);
        let rendered = p.slope * (frame.white_stops + e0 + p.exposure);
        assert!((rendered - FRAME_WHITE_LOOK_STOPS).abs() < 1e-5, "{p:?}");
        // A night frame's white wants 3.5 EV of lift: held at the limit over the roll's.
        let p = per_frame_placement(stops(-3.0, -4.0, -6.0), e0);
        assert_eq!(p.exposure_bound, ExposureBound::Limit);
        assert_eq!(p.exposure, FRAME_EXPOSURE_ABOVE_EV);
        // A white far over the target is darkened at most the limit under the roll's.
        let p = per_frame_placement(stops(4.0, 0.0, -1.0), e0);
        assert_eq!(p.exposure_bound, ExposureBound::Limit);
        assert_eq!(p.exposure, -FRAME_EXPOSURE_BELOW_EV);
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
    fn white_over_mid_is_the_log_of_the_ratio() {
        // In f64, so the check does not rest on this target's `log2f`.
        let exact = (f64::from(DIFFUSE_WHITE) / f64::from(MID_GREY)).log2() as f32;
        assert_eq!(WHITE_OVER_MID_STOPS.to_bits(), exact.to_bits());
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

    /// A ramp of film-RGB pixels from mid-grey to +3 stops, tinted per channel.
    fn ramp(tint: [f32; 3]) -> Vec<f32> {
        (0..1001)
            .flat_map(|i| {
                let v = at(3.0 * i as f32 / 1000.0);
                tint.map(|t| v * t)
            })
            .collect()
    }

    #[test]
    fn with_unit_gains_and_no_line_the_corrected_white_is_the_decoded_one() {
        let s = ramp([1.0, 0.8, 0.6]);
        let (w, c) = (
            sample_white(&s).unwrap(),
            corrected_white(&s, &RollBalance::Gain([1.0; 3]), None).unwrap(),
        );
        // Only the 3×3 round trip's f32 roundings separate them.
        assert!((c / w - 1.0).abs() < 1e-5, "{w} {c}");
    }

    #[test]
    fn the_corrected_white_follows_the_gains() {
        let grey = ramp([1.0; 3]);
        let w = sample_white(&grey).unwrap();
        // Equal gains scale every channel: the white moves by log2 of the gain.
        let c = corrected_white(&grey, &RollBalance::Gain([1.5; 3]), None).unwrap();
        assert!((scene_stops(c) - scene_stops(w) - 1.5f32.log2()).abs() < 1e-5);
        // Unequal gains act in ACEScg: the white is the brightest film channel of the
        // corrected p97 pixel, mapped back.
        let gains = [1.3, 1.0, 0.8];
        let aces = film_rgb_to_acescg([w; 3]);
        let back = acescg_to_film_rgb(std::array::from_fn(|c| aces[c] * gains[c]));
        let c = corrected_white(&grey, &RollBalance::Gain(gains), None).unwrap();
        assert!(
            (c / back[0].max(back[1]).max(back[2]) - 1.0).abs() < 1e-5,
            "{c} {back:?}"
        );
    }

    #[test]
    fn the_pooled_white_is_the_scene_stop_of_the_balanced_lumas_p99() {
        let px = [0.5, 0.4, 0.3];
        let gains = [0.8, 1.0, 1.4];
        let samples = vec![field(px, 50), field(px, 30)];
        let luma: f32 = (0..3).map(|c| ACESCG_LUMA[c] * px[c] * gains[c]).sum();
        let w = pooled_white_stops(&samples, &RollBalance::Gain(gains));
        assert!((w - scene_stops(luma)).abs() < 1e-6, "{w}");
        // Read without a flattened copy, bit for bit as the copy read.
        let samples = vec![
            ramp([1.0, 0.8, 0.6]),
            field([f32::NAN, 0.3, 0.3], 5),
            ramp([0.5; 3]),
        ];
        let flat: Vec<f32> = samples
            .iter()
            .flat_map(|s| s.as_chunks::<3>().0.iter())
            .filter(|px| !unusable(px))
            .flatten()
            .copied()
            .collect();
        let p = percentile_levels(&flat, PERCENTILE);
        let want = scene_stops((0..3).map(|c| ACESCG_LUMA[c] * p[c] * gains[c]).sum());
        assert_eq!(
            pooled_white_stops(&samples, &RollBalance::Gain(gains)).to_bits(),
            want.to_bits()
        );
    }

    #[test]
    fn a_bright_frame_far_from_its_leader_does_not_warn() {
        let d = leader_distance_stops(at(2.7), at(4.5));
        assert!((d - 1.8).abs() < 1e-4, "{d}");
        assert!(!near_saturation(d));
    }
}
