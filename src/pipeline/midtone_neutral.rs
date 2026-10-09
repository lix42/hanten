//! **The midtone neutral** (`nf-scene-correction/midtone-neutral`): the cast a poor
//! development leaves in the midtones, measured once per roll by `measure-roll` and
//! removed in scene correction.
//!
//! The roll white balance ([`super::roll_white`]) is exact at the roll's p99 only; on a
//! roll from an exhausted developer the cast grows away from it. Per band of scene
//! brightness, each frame votes the densest cluster of its colour ratios after the roll
//! white balance, the roll's value per band is the median vote, and a weighted line
//! through the bands gives red and blue a correction against scene stops
//! ([`measure`]). The evidence and the reviewed definitions are
//! `docs/spike/poor-development.md` ("As reviewed").
//!
//! At render ([`MidtoneLine::correct`]) the line applies in full up to [`FADE_STOPS`]
//! below the roll's white, fades to zero at it, and is flat outside the voted bands;
//! luminance is restored per pixel. A **tint gate** spares strongly coloured light
//! (a sunset-lit cloud): the correction fades out as a pixel's colour moves
//! [`GATE_LOG2`] from the line's cast at its stop.
//!
//! **The switch is a data floor, not a detector**: no statistic separated poor
//! development from good, and the line did no visible harm on good rolls, so it is on
//! unless the roll has fewer than [`MIN_FRAMES`] frames or [`MIN_BANDS`] voted bands.
//! What it cannot tell from a cast is a roll dominated by one scene colour (beach, sky),
//! and no statistic found does ([`super::correction_confidence`]).
//!
//! Its values (band width, cluster radius, fade width, gate) are the spike's, reviewed
//! on four rolls; `nf-scene-correction/midtone-neutral-fit` settles the fit range and fade
//! width against ColorChecker frames.

use serde::{Deserialize, Serialize};

use crate::pipeline::colorimetry::dot;
use crate::pipeline::colorimetry::pinned::ACESCG_LUMA;
use crate::pipeline::look::MID_GREY;
use crate::pipeline::roll_white::scene_stops;

/// The lowest band edge, in scene stops (log2 of ACEScg luma over mid-grey).
pub const BAND_LOW_STOPS: f32 = -3.0;
/// Each band's width in scene stops.
pub const BAND_WIDTH_STOPS: f32 = 0.5;
/// Bands from [`BAND_LOW_STOPS`] to +2.5 stops.
pub const BAND_COUNT: usize = 11;
/// The share of a frame's sample it needs in a band to vote there: the spike's 300 of
/// about 225,000 sampled pixels, as a share so the rule does not move with the sample size.
pub const BAND_MIN_SHARE: f64 = 300.0 / 225_000.0;
/// The cluster's radius in log2 colour ratio: the densest cluster, not the band's
/// median, so a coloured object sharing the band does not pull the vote.
pub const CLUSTER_RADIUS: f64 = 0.15;
/// Most re-centring steps of a vote's cluster.
pub const CLUSTER_STEPS: usize = 5;
/// Re-centring stops when the cluster keeps fewer than this share of the band's floor.
pub const CLUSTER_MIN_OF_FLOOR: f64 = 0.25;
/// Fewest frames voting for a band to count.
pub const BAND_MIN_VOTES: usize = 3;
/// Fewest frames for the line to be on by default: the three- and four-frame rolls lost
/// clearly in review.
pub const MIN_FRAMES: usize = 10;
/// Fewest counted bands to fit the line. Not reviewed: a line through two points has
/// no check, and every reviewed roll counted nine or more.
pub const MIN_BANDS: usize = 3;
/// How far below the roll's white the fade starts, in scene stops.
pub const FADE_STOPS: f32 = 1.0;
/// The tint gate: full correction within the first distance (log2 colour ratio) of the
/// line's cast, none beyond the second. True neutrals sat within ~0.23 on the reviewed
/// rolls, 09-20 1883's sunset-lit underside at 0.73.
pub const GATE_LOG2: [f32; 2] = [0.6, 0.9];

/// The measured line — the recipe's `roll.midtone_line`. Each channel's correction is
/// `slope · s + offset`, in log2 of its ratio to green, at scene stop `s` after the roll
/// white balance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidtoneLine {
    /// Red's `[slope, offset]`.
    pub red: [f32; 2],
    /// Blue's `[slope, offset]`.
    pub blue: [f32; 2],
    /// The lowest and highest counted band centres, in scene stops: the line is flat
    /// outside them.
    pub bands: [f32; 2],
    /// The roll's white as the line sees it — the scene stop of the roll white balance's
    /// pooled p99 — where the correction has faded to zero. Not `roll.white_stops`,
    /// which is measured after this correction.
    pub fade_end_stops: f32,
}

impl MidtoneLine {
    /// Every value finite and the bands ordered; the one value rule.
    pub fn check(&self) -> Result<(), String> {
        let values = [
            self.red[0],
            self.red[1],
            self.blue[0],
            self.blue[1],
            self.bands[0],
            self.bands[1],
            self.fade_end_stops,
        ];
        if values.iter().any(|v| !v.is_finite()) {
            return Err(format!("every value must be finite, got {self:?}"));
        }
        if self.bands[0] > self.bands[1] {
            return Err(format!(
                "its bands must be [low, high], got {:?}",
                self.bands
            ));
        }
        Ok(())
    }

    /// The `[red, blue]` cast at scene stop `s`, fade included.
    fn cast(&self, s: f32) -> [f32; 2] {
        let join = self.fade_end_stops - FADE_STOPS;
        let fade = if s > join {
            ((self.fade_end_stops - s) / FADE_STOPS).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let at = s.min(join).clamp(self.bands[0], self.bands[1]);
        [
            (self.red[0] * at + self.red[1]) * fade,
            (self.blue[0] * at + self.blue[1]) * fade,
        ]
    }

    /// Remove the cast from one linear ACEScg pixel, **before** the white balance:
    /// `gains` are the roll's, which the line was measured after. Red and blue are
    /// divided by the cast, then all three scaled back to the pixel's luminance. A pixel
    /// with no finite positive luminance is left alone.
    pub fn correct(&self, px: &mut [f32; 3], gains: [f32; 3]) {
        let luma = |p: [f32; 3]| dot(p, ACESCG_LUMA);
        let balanced = [px[0] * gains[0], px[1] * gains[1], px[2] * gains[2]];
        let y = luma(*px);
        let yw = luma(balanced);
        if !(y > 0.0 && yw > 0.0 && y.is_finite() && yw.is_finite()) {
            return;
        }
        let [cr, cb] = self.cast(scene_stops(yw));
        let off_cast = ((balanced[0] / balanced[1]).log2() - cr)
            .hypot((balanced[2] / balanced[1]).log2() - cb);
        // A channel at zero or below has no ratio; the spike corrected it in full.
        let gate = if off_cast.is_finite() {
            ((GATE_LOG2[1] - off_cast) / (GATE_LOG2[1] - GATE_LOG2[0])).clamp(0.0, 1.0)
        } else {
            1.0
        };
        px[0] *= (-cr * gate).exp2();
        px[2] *= (-cb * gate).exp2();
        let y2 = luma(*px);
        if y2 > 0.0 {
            let k = y / y2;
            for c in px.iter_mut() {
                *c *= k;
            }
        }
    }
}

/// One counted band, for `measure-roll`'s report.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Band {
    /// The band's centre, in scene stops.
    pub centre: f32,
    /// Frames voting.
    pub votes: usize,
    /// The median vote, `[log2 r/g, log2 b/g]`.
    pub median: [f32; 2],
}

/// What [`measure`] found.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Measured {
    /// The line, or `None` when the data floor (or too few bands) left it off.
    pub line: Option<MidtoneLine>,
    /// Frames that contributed samples.
    pub frames: usize,
    /// The counted bands.
    pub bands: Vec<Band>,
    /// Why there is no line, when there is none.
    pub off_because: Option<OffBecause>,
}

impl Measured {
    /// `--midtone-neutral off`: nothing measured over `frames` contributing frames.
    pub fn asked(frames: usize) -> Self {
        Self {
            line: None,
            frames,
            bands: Vec::new(),
            off_because: Some(OffBecause::Asked),
        }
    }
}

/// Why a roll gets no line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OffBecause {
    /// `measure-roll --midtone-neutral off`.
    Asked,
    /// Fewer than [`MIN_FRAMES`] frames, under `auto`.
    TooFewFrames,
    /// Fewer than [`MIN_BANDS`] counted bands.
    TooFewBands,
}

/// Measure the line over each frame's samples in linear ACEScg **before** the roll white
/// balance (`gains`), fading to zero at `fade_end_stops`
/// (`roll_white::pooled_white_stops`). `floor` applies [`MIN_FRAMES`] (`auto`); the band
/// floor always applies. Deterministic: each statistic reads its samples in order.
pub fn measure(frames: &[&[f32]], gains: [f32; 3], fade_end_stops: f32, floor: bool) -> Measured {
    let contributing: Vec<&[f32]> = frames.iter().copied().filter(|f| !f.is_empty()).collect();
    let mut votes: Vec<Vec<[f64; 2]>> = vec![Vec::new(); BAND_COUNT];
    for frame in &contributing {
        for (band, vote) in frame_votes(frame, gains).into_iter().enumerate() {
            votes[band].extend(vote);
        }
    }
    let bands: Vec<Band> = votes
        .iter()
        .enumerate()
        .filter(|(_, v)| v.len() >= BAND_MIN_VOTES)
        .map(|(band, v)| Band {
            centre: band_centre(band),
            votes: v.len(),
            median: median2(v).map(|m| m as f32),
        })
        .collect();
    let off_because = if floor && contributing.len() < MIN_FRAMES {
        Some(OffBecause::TooFewFrames)
    } else if bands.len() < MIN_BANDS {
        Some(OffBecause::TooFewBands)
    } else {
        None
    };
    let line = off_because.is_none().then(|| MidtoneLine {
        red: weighted_line(&bands, 0),
        blue: weighted_line(&bands, 1),
        bands: [bands[0].centre, bands[bands.len() - 1].centre],
        fade_end_stops,
    });
    Measured {
        line,
        frames: contributing.len(),
        bands,
        off_because,
    }
}

fn band_centre(band: usize) -> f32 {
    BAND_LOW_STOPS + BAND_WIDTH_STOPS * (band as f32 + 0.5)
}

/// One frame's vote per band, if it has [`BAND_MIN_SHARE`] of its sample there: the centre
/// of the densest cluster of `[log2 r/g, log2 b/g]` after `gains`.
fn frame_votes(frame: &[f32], gains: [f32; 3]) -> Vec<Option<[f64; 2]>> {
    let floor = ((frame.len() / 3) as f64 * BAND_MIN_SHARE).ceil() as usize;
    let cluster_floor = (floor as f64 * CLUSTER_MIN_OF_FLOOR).ceil() as usize;
    let mut by_band: Vec<Vec<[f64; 2]>> = vec![Vec::new(); BAND_COUNT];
    for px in frame.as_chunks::<3>().0 {
        let b = [px[0] * gains[0], px[1] * gains[1], px[2] * gains[2]];
        if b.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
            continue;
        }
        let s = (f64::from(dot(b, ACESCG_LUMA)) / f64::from(MID_GREY)).log2();
        let at = ((s - f64::from(BAND_LOW_STOPS)) / f64::from(BAND_WIDTH_STOPS)).floor();
        if at >= 0.0 && at < BAND_COUNT as f64 {
            let (r, g, bl) = (f64::from(b[0]), f64::from(b[1]), f64::from(b[2]));
            by_band[at as usize].push([(r / g).log2(), (bl / g).log2()]);
        }
    }
    by_band
        .iter()
        .map(|c| (c.len() >= floor).then(|| densest_cluster(c, cluster_floor)))
        .collect()
}

/// Start at the median, then re-centre on the median of the points within
/// [`CLUSTER_RADIUS`], up to [`CLUSTER_STEPS`] times, while `min` points remain.
fn densest_cluster(points: &[[f64; 2]], min: usize) -> [f64; 2] {
    let mut centre = median2(points);
    for _ in 0..CLUSTER_STEPS {
        let near: Vec<[f64; 2]> = points
            .iter()
            .copied()
            .filter(|p| (p[0] - centre[0]).hypot(p[1] - centre[1]) < CLUSTER_RADIUS)
            .collect();
        if near.len() < min {
            break;
        }
        centre = median2(&near);
    }
    centre
}

/// The per-axis median of a non-empty set; the mean of the middle two when even. By
/// selection, not a sort: the same elements, since `total_cmp` is a total order.
fn median2(points: &[[f64; 2]]) -> [f64; 2] {
    let n = points.len();
    let mut v = Vec::with_capacity(n);
    std::array::from_fn(|k| {
        v.clear();
        v.extend(points.iter().map(|p| p[k]));
        let (below, &mut mid, _) = v.select_nth_unstable_by(n / 2, f64::total_cmp);
        if n % 2 == 1 {
            mid
        } else {
            let lower = below
                .iter()
                .copied()
                .max_by(f64::total_cmp)
                .expect("n is even and non-zero");
            (lower + mid) / 2.0
        }
    })
}

/// Least squares `[slope, offset]` through the bands' medians on `axis`, each band
/// weighted by its votes (the spike's `polyfit` with `w = √votes`).
fn weighted_line(bands: &[Band], axis: usize) -> [f32; 2] {
    let w: Vec<f64> = bands.iter().map(|b| b.votes as f64).collect();
    let x: Vec<f64> = bands.iter().map(|b| f64::from(b.centre)).collect();
    let y: Vec<f64> = bands.iter().map(|b| f64::from(b.median[axis])).collect();
    let sw: f64 = w.iter().sum();
    let mx = w.iter().zip(&x).map(|(w, x)| w * x).sum::<f64>() / sw;
    let my = w.iter().zip(&y).map(|(w, y)| w * y).sum::<f64>() / sw;
    let sxy: f64 = (0..w.len()).map(|i| w[i] * (x[i] - mx) * (y[i] - my)).sum();
    let sxx: f64 = (0..w.len()).map(|i| w[i] * (x[i] - mx).powi(2)).sum();
    let slope = sxy / sxx;
    [slope as f32, (my - slope * mx) as f32]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A neutral-then-cast pixel at scene stop `s`: luma `0.18·2^s`, with
    /// `log2 r/g = rho[0]`, `log2 b/g = rho[1]`.
    fn pixel(s: f64, rho: [f64; 2]) -> [f32; 3] {
        let (r, b) = (rho[0].exp2(), rho[1].exp2());
        let l = ACESCG_LUMA.map(f64::from);
        let g = 0.18 * s.exp2() / (l[0] * r + l[1] + l[2] * b);
        [(g * r) as f32, g as f32, (g * b) as f32]
    }

    fn line() -> MidtoneLine {
        MidtoneLine {
            red: [-0.05, 0.04],
            blue: [-0.02, 0.37],
            bands: [-2.75, 1.75],
            fade_end_stops: 1.9,
        }
    }

    /// Frames whose every band carries the cast `rho(s)`, with a coloured object sharing
    /// each band that the cluster must ignore.
    fn frames(n: usize, rho: impl Fn(f64) -> [f64; 2]) -> Vec<Vec<f32>> {
        (0..n)
            .map(|f| {
                let mut v = Vec::new();
                for band in 0..BAND_COUNT {
                    let s = f64::from(band_centre(band)) + 0.01 * f as f64;
                    for k in 0..400 {
                        let jitter = ((k % 7) as f64 - 3.0) * 0.002;
                        let c = rho(s);
                        v.extend(pixel(s, [c[0] + jitter, c[1] - jitter]));
                    }
                    for _ in 0..150 {
                        v.extend(pixel(s, [0.8, -0.6]));
                    }
                }
                v
            })
            .collect()
    }

    #[test]
    fn a_cast_linear_in_stops_is_measured_as_its_line() {
        let cast = |s: f64| [-0.06 * s + 0.04, -0.02 * s + 0.37];
        let f = frames(12, cast);
        let refs: Vec<&[f32]> = f.iter().map(Vec::as_slice).collect();
        let m = measure(&refs, [1.0; 3], 1.9, true);
        let l = m.line.expect("twelve frames and eleven bands");
        assert_eq!(m.bands.len(), BAND_COUNT);
        assert!(
            (l.red[0] + 0.06).abs() < 0.003 && (l.red[1] - 0.04).abs() < 0.006,
            "{l:?}"
        );
        assert!(
            (l.blue[0] + 0.02).abs() < 0.003 && (l.blue[1] - 0.37).abs() < 0.006,
            "{l:?}"
        );
        assert_eq!(l.bands, [-2.75, 2.25]);
        assert!((l.fade_end_stops - 1.9).abs() < 1e-5);
    }

    #[test]
    fn the_data_floor_and_the_band_floor_leave_it_off() {
        let f = frames(9, |_| [0.0, 0.2]);
        let refs: Vec<&[f32]> = f.iter().map(Vec::as_slice).collect();
        let auto = measure(&refs, [1.0; 3], 1.0, true);
        assert_eq!(
            (auto.line, auto.off_because),
            (None, Some(OffBecause::TooFewFrames))
        );
        assert!(
            measure(&refs, [1.0; 3], 1.0, false).line.is_some(),
            "`on` skips the frame floor"
        );
        // Two frames per band never count.
        let two = measure(&refs[..2], [1.0; 3], 1.0, false);
        assert_eq!(two.off_because, Some(OffBecause::TooFewBands));
    }

    #[test]
    fn the_correction_neutralises_the_cast_and_keeps_luminance() {
        let l = line();
        for s in [-2.0, -0.5, 0.0, 0.6] {
            let mut px = pixel(s, l.cast(s as f32).map(f64::from));
            let y = ACESCG_LUMA.iter().zip(px).map(|(w, v)| w * v).sum::<f32>();
            l.correct(&mut px, [1.0; 3]);
            let y2 = ACESCG_LUMA.iter().zip(px).map(|(w, v)| w * v).sum::<f32>();
            assert!(
                (px[0] / px[1] - 1.0).abs() < 1e-5 && (px[2] / px[1] - 1.0).abs() < 1e-5,
                "{s}: {px:?}"
            );
            assert!((y2 / y - 1.0).abs() < 1e-5, "{s}");
        }
    }

    #[test]
    fn the_fade_and_the_bands_bound_the_cast() {
        let l = line();
        // Full below the join, half in the middle of the fade, none at and above white.
        let join = l.fade_end_stops - FADE_STOPS;
        assert_eq!(
            l.cast(join),
            [l.red[0] * join + l.red[1], l.blue[0] * join + l.blue[1]]
        );
        let half = l.cast(join + 0.5);
        let full = l.cast(join);
        assert!((half[1] - full[1] / 2.0).abs() < 1e-6);
        assert_eq!(l.cast(l.fade_end_stops), [0.0, 0.0]);
        assert_eq!(l.cast(4.0), [0.0, 0.0]);
        // Flat below the lowest band.
        assert_eq!(l.cast(-6.0), l.cast(l.bands[0]));
    }

    #[test]
    fn the_median_by_selection_is_the_sorted_median() {
        let sorted = |v: &mut Vec<f64>| {
            v.sort_unstable_by(f64::total_cmp);
            let n = v.len();
            if n % 2 == 1 {
                v[n / 2]
            } else {
                (v[n / 2 - 1] + v[n / 2]) / 2.0
            }
        };
        for n in 1..40usize {
            // Duplicates and both zeros included.
            let points: Vec<[f64; 2]> = (0..n)
                .map(|i| {
                    let a = ((i * 37 % 11) as f64 - 5.0) * 0.1;
                    [if a == 0.0 { -0.0 } else { a }, ((i * 13) % 7) as f64 * 0.3]
                })
                .collect();
            let got = median2(&points);
            for k in 0..2 {
                let want = sorted(&mut points.iter().map(|p| p[k]).collect());
                assert_eq!(got[k].to_bits(), want.to_bits(), "n {n} axis {k}");
            }
        }
    }

    #[test]
    fn a_join_below_the_bands_still_reads_the_line_inside_them() {
        // A white so low the fade starts under the lowest band.
        let l = MidtoneLine {
            fade_end_stops: -2.0,
            ..line()
        };
        let low = l.bands[0];
        let at_low = [l.red[0] * low + l.red[1], l.blue[0] * low + l.blue[1]];
        assert_eq!(l.cast(-4.0), at_low);
        let fade = (l.fade_end_stops - -2.5) / FADE_STOPS;
        let c = l.cast(-2.5);
        assert!(
            (c[0] - at_low[0] * fade).abs() < 1e-6 && (c[1] - at_low[1] * fade).abs() < 1e-6,
            "{c:?}"
        );
    }

    #[test]
    fn the_tint_gate_spares_strongly_coloured_light() {
        let l = line();
        let s = 0.0_f64;
        let cast = l.cast(s as f32).map(f64::from);
        let at = |d: f64| {
            let mut px = pixel(s, [cast[0] + d, cast[1]]);
            let before = px;
            l.correct(&mut px, [1.0; 3]);
            (px[2] / px[1]).log2() - (before[2] / before[1]).log2()
        };
        let full = at(0.0);
        assert!(
            (full + cast[1] as f32).abs() < 1e-4,
            "inside the gate: full correction"
        );
        assert!(
            (at(0.75) - full / 2.0).abs() < 1e-3,
            "halfway through the gate: half"
        );
        assert!(at(1.0).abs() < 1e-6, "beyond the gate: none");
    }

    #[test]
    fn a_pixel_without_finite_positive_luminance_is_left_alone() {
        for px in [
            [0.0, 0.0, 0.0],
            [-0.1, 0.01, 0.0],
            [f32::NAN, 0.2, 0.2],
            [f32::INFINITY, 0.1, 0.1],
            [0.1, 0.1, f32::NAN],
        ] {
            let mut out = px;
            line().correct(&mut out, [1.0; 3]);
            assert_eq!(out.map(f32::to_bits), px.map(f32::to_bits));
        }
    }

    #[test]
    fn a_line_with_a_non_finite_value_or_reversed_bands_is_refused() {
        assert!(line().check().is_ok());
        assert!(
            MidtoneLine {
                red: [f32::NAN, 0.0],
                ..line()
            }
            .check()
            .is_err()
        );
        assert!(
            MidtoneLine {
                bands: [1.0, -1.0],
                ..line()
            }
            .check()
            .is_err()
        );
    }
}
