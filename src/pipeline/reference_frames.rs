//! **Finding a roll's reference frames among its inputs** (`core/auto-calibration`):
//! `measure-roll --unexposed auto` and `--leader auto` classify every input from its
//! effective-area median and spread ([`super::film_base::measure_area`]).
//!
//! - **Unexposed:** flat (spread below [`UNEXPOSED_MAX_SPREAD`]) and as clear as the
//!   clearest such frame, within [`AGREEMENT_DENSITY`] on every channel. Unexposed film
//!   is the clearest thing on a roll, so a flat candidate denser than another is a
//!   near-blank picture, and one that any input beats is no base at all — with no
//!   unexposed frame among the inputs, the leader or a flat picture is the clearest flat
//!   frame. Two or more agreeing frames corroborate the base; one does not.
//! - **Leader:** flat by the looser picture guard
//!   ([`super::film_base::AREA_MAX_RELATIVE_SPREAD`]) and at least
//!   [`LEADER_MIN_DENSITY`] denser than the roll's base on every channel — measured
//!   against the base, never an absolute level, which moves with the scan's exposure.
//!
//! The constants and their evidence: `docs/progress/core.md`, `auto-calibration`.

use serde::Serialize;

/// An unexposed candidate's area spread is below this. Over the archive's 243 frames,
/// the 11 unexposed read 0.06-0.28 and the flattest picture 0.30.
pub const UNEXPOSED_MAX_SPREAD: f32 = 0.30;

/// How far, in density on any channel, a candidate may sit from the clearest one and
/// still be the same unexposed film, and how far another input may be clearer than it.
/// Two blank frames of one roll agree to 0.003; the nearest near-blank picture sits
/// 0.024 off. With a roll's unexposed frame left out, every other input it then picks is
/// beaten by 0.2 or more; with it in, nothing comes within 0.02 of beating it.
pub const AGREEMENT_DENSITY: f32 = 0.01;

/// Least density above the base, on every channel, for a leader. Leaders read
/// 1.02-1.47; flat pictures at most 0.40.
pub const LEADER_MIN_DENSITY: f32 = 0.7;

/// One input's effective-area measurement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AreaStats {
    /// The per-channel median transmission.
    pub median: [f32; 3],
    /// The worst per-channel `(p90 - p10) / p50`.
    pub spread: f32,
}

/// Whether a detected base rests on more than one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    Corroborated,
    Uncorroborated,
}

/// The unexposed frames found among the inputs.
#[derive(Clone, Debug, PartialEq)]
pub struct Unexposed {
    /// The agreeing frames, by input index, in input order.
    pub frames: Vec<usize>,
    /// Flat candidates denser than the clearest: near-blank pictures.
    pub rejected: Vec<usize>,
    /// The per-channel median of the agreeing frames' medians.
    pub base: [f32; 3],
    pub confidence: Confidence,
    /// The widest per-channel density range across the agreeing frames.
    pub spread_density: f32,
}

/// Why no unexposed frame was found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NotFound {
    /// No input is flat enough; the flattest measured one's spread.
    NoneFlat { flattest: Option<f32> },
    /// The clearest flat input is beaten by a clearer one, by this much density on
    /// every channel at least.
    Beaten {
        candidate: usize,
        by: usize,
        density: f32,
    },
}

/// Per-channel density of `median` above `base`: `log10(base / median)`.
pub fn density_above(base: [f32; 3], median: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|c| (base[c] / median[c]).log10())
}

fn mean_density(median: [f32; 3]) -> f32 {
    -median.iter().map(|m| m.log10()).sum::<f32>() / 3.0
}

/// Frame `i`'s measurement; only called on an index known to be measured.
fn at(frames: &[Option<AreaStats>], i: usize) -> AreaStats {
    frames[i].expect("a measured frame")
}

/// The clearest of measured `indices` by mean density; the first on a tie.
fn clearest(frames: &[Option<AreaStats>], indices: &[usize]) -> Option<usize> {
    indices.iter().copied().reduce(|a, b| {
        if mean_density(at(frames, b).median) < mean_density(at(frames, a).median) {
            b
        } else {
            a
        }
    })
}

/// Find the unexposed frames among the inputs; `None` is an input whose area could not
/// be measured, which is no candidate.
pub fn find_unexposed(frames: &[Option<AreaStats>]) -> Result<Unexposed, NotFound> {
    let measured: Vec<usize> = (0..frames.len()).filter(|&i| frames[i].is_some()).collect();
    let candidates: Vec<usize> = measured
        .iter()
        .copied()
        .filter(|&i| at(frames, i).spread < UNEXPOSED_MAX_SPREAD)
        .collect();
    let Some(candidate) = clearest(frames, &candidates) else {
        let flattest = measured
            .iter()
            .map(|&i| at(frames, i).spread)
            .reduce(f32::min);
        return Err(NotFound::NoneFlat { flattest });
    };
    let clear = at(frames, candidate).median;
    // How much clearer than the candidate each input is, on its least clear channel.
    let beaten = |j: usize| {
        -density_above(clear, at(frames, j).median)
            .into_iter()
            .fold(f32::NEG_INFINITY, f32::max)
    };
    if let Some(by) = measured
        .iter()
        .copied()
        .max_by(|&a, &b| beaten(a).total_cmp(&beaten(b)))
        && beaten(by) > AGREEMENT_DENSITY
    {
        return Err(NotFound::Beaten {
            candidate,
            by,
            density: beaten(by),
        });
    }
    let (agreeing, rejected): (Vec<usize>, Vec<usize>) = candidates.into_iter().partition(|&i| {
        density_above(clear, at(frames, i).median)
            .iter()
            .all(|d| d.abs() <= AGREEMENT_DENSITY)
    });
    let base = std::array::from_fn(|c| {
        let mut v: Vec<f32> = agreeing.iter().map(|&i| at(frames, i).median[c]).collect();
        super::roll_white::median(&mut v)
    });
    let spread_density = (0..3)
        .map(|c| {
            let d = agreeing.iter().map(|&i| -at(frames, i).median[c].log10());
            let (lo, hi) = d.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| {
                (lo.min(x), hi.max(x))
            });
            hi - lo
        })
        .fold(0.0, f32::max);
    Ok(Unexposed {
        confidence: if agreeing.len() >= 2 {
            Confidence::Corroborated
        } else {
            Confidence::Uncorroborated
        },
        frames: agreeing,
        rejected,
        base,
        spread_density,
    })
}

/// Whether a frame reads as a leader against the roll's `base`.
pub fn is_leader(frame: &AreaStats, base: [f32; 3]) -> bool {
    frame.spread <= super::film_base::AREA_MAX_RELATIVE_SPREAD
        && density_above(base, frame.median)
            .iter()
            .all(|&d| d >= LEADER_MIN_DENSITY)
}

/// The leader-like frames among `indices`, and the one to guard with: the least dense,
/// whose guard leaves out the most pixels.
pub fn find_leaders(
    frames: &[Option<AreaStats>],
    indices: impl IntoIterator<Item = usize>,
    base: [f32; 3],
) -> Option<(usize, Vec<usize>)> {
    let leaders: Vec<usize> = indices
        .into_iter()
        .filter(|&i| frames[i].is_some_and(|f| is_leader(&f, base)))
        .collect();
    Some((clearest(frames, &leaders)?, leaders))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(density: [f32; 3], spread: f32) -> Option<AreaStats> {
        Some(AreaStats {
            median: density.map(|d| 10f32.powf(-d)),
            spread,
        })
    }

    fn stat(density: [f32; 3], spread: f32) -> AreaStats {
        at(density, spread).unwrap()
    }

    const BASE: [f32; 3] = [0.3, 0.6, 0.75];

    fn above(d: f32) -> [f32; 3] {
        BASE.map(|b| b + d)
    }

    #[test]
    fn one_unexposed_frame_is_uncorroborated() {
        let frames = [at(above(0.6), 1.4), at(BASE, 0.15), at(above(0.3), 0.9)];
        let u = find_unexposed(&frames).unwrap();
        assert_eq!(u.frames, [1]);
        assert_eq!(u.confidence, Confidence::Uncorroborated);
        assert_eq!(u.base, frames[1].unwrap().median);
        assert_eq!(u.spread_density, 0.0);
    }

    #[test]
    fn agreeing_frames_corroborate_and_report_their_spread() {
        let frames = [at(BASE, 0.15), at(above(0.6), 1.4), at(above(0.004), 0.12)];
        let u = find_unexposed(&frames).unwrap();
        assert_eq!(u.frames, [0, 2]);
        assert_eq!(u.confidence, Confidence::Corroborated);
        assert!(
            (u.spread_density - 0.004).abs() < 1e-4,
            "{}",
            u.spread_density
        );
        for c in 0..3 {
            let mid = (frames[0].unwrap().median[c] + frames[2].unwrap().median[c]) / 2.0;
            assert_eq!(u.base[c], mid);
        }
    }

    #[test]
    fn a_near_blank_picture_is_rejected_beside_the_clearer_base() {
        // `1698`: flat, and 0.03 denser than its roll's base on every channel.
        let frames = [at(above(0.03), 0.28), at(BASE, 0.2)];
        let u = find_unexposed(&frames).unwrap();
        assert_eq!(u.frames, [1]);
        assert_eq!(u.rejected, [0]);
    }

    #[test]
    fn a_frame_at_the_spread_gate_is_never_a_candidate() {
        // The flattest archive picture reads 0.30 and a near-blank one 0.34.
        assert_eq!(
            find_unexposed(&[at(BASE, 0.34), at(above(0.5), 1.0)]),
            Err(NotFound::NoneFlat {
                flattest: Some(0.34)
            })
        );
        assert!(find_unexposed(&[at(BASE, UNEXPOSED_MAX_SPREAD)]).is_err());
    }

    #[test]
    fn an_unmeasured_frame_is_no_candidate() {
        assert_eq!(
            find_unexposed(&[None]),
            Err(NotFound::NoneFlat { flattest: None })
        );
        assert_eq!(find_unexposed(&[None, at(BASE, 0.1)]).unwrap().frames, [1]);
        let base = BASE.map(|d| 10f32.powf(-d));
        assert_eq!(find_leaders(&[None], [0], base), None);
    }

    #[test]
    fn a_flat_frame_a_picture_beats_is_no_base() {
        // No unexposed frame among the inputs: the leader is the clearest flat frame,
        // and a picture's median is far clearer than it.
        let frames = [
            at(above(1.2), 0.23),
            at(above(0.4), 1.3),
            at(above(0.5), 1.6),
        ];
        let Err(NotFound::Beaten {
            candidate,
            by,
            density,
        }) = find_unexposed(&frames)
        else {
            panic!("{:?}", find_unexposed(&frames));
        };
        assert_eq!((candidate, by), (0, 1));
        assert!((density - 0.8).abs() < 1e-4, "{density}");
        // A frame clearer on one channel only does not beat the base (`1641`).
        let mut one = BASE;
        one[0] -= 0.02;
        assert!(find_unexposed(&[at(BASE, 0.1), at(one, 0.47)]).is_ok());
    }

    #[test]
    fn the_input_order_does_not_move_the_base() {
        let a = [at(BASE, 0.1), at(above(0.005), 0.1), at(above(-0.003), 0.1)];
        let b = [a[2], a[0], a[1]];
        assert_eq!(
            find_unexposed(&a).unwrap().base,
            find_unexposed(&b).unwrap().base
        );
    }

    #[test]
    fn a_leader_is_dense_against_the_base_on_every_channel() {
        let base = at(BASE, 0.1).unwrap().median;
        assert!(is_leader(&stat(above(1.1), 0.3), base));
        // Flat pictures: dense on two channels, not the third.
        let mut flat = above(0.9);
        flat[0] = BASE[0] + 0.4;
        assert!(!is_leader(&stat(flat, 0.3), base));
        // Dense but not flat: a dark picture.
        assert!(!is_leader(&stat(above(1.1), 0.9), base));
    }

    #[test]
    fn the_least_dense_leader_guards() {
        let base = at(BASE, 0.1).unwrap().median;
        let frames = [
            at(above(1.4), 0.3),
            at(above(0.2), 1.0),
            at(above(1.1), 0.4),
        ];
        assert_eq!(find_leaders(&frames, 0..3, base), Some((2, vec![0, 2])));
        assert_eq!(find_leaders(&frames, [1], base), None);
    }
}
