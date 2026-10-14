//! Near-black collapse (`algo/near-black-collapse-warning`): a rendered frame whose
//! brightest content sits far below diffuse white on some channel.
//!
//! The encode's black-channel rule (`OutputStats::black_channels`) fires only when a
//! channel writes nothing above 0. A render that is black in practice can still keep
//! tiny codes, float values or PQ's floor, so this rule reads the **display-referred
//! linear buffer** the chain hands the encoder instead, where diffuse white
//! ([`DIFFUSE_WHITE`]) is the same for every destination. Below white the SDR and HDR
//! renditions agree (`pipeline::chain`'s branch contract), so one threshold serves them
//! all. The film master has no display white and is not measured.
//!
//! The measure is each channel's [`PERCENTILE`], not its maximum, so a dust speck or a
//! specular glint cannot hide a collapsed frame. It is read off a histogram keyed by the
//! sample's f32 bits — exponent plus the top mantissa bits — so it needs no `log2`, its
//! integer counts fold in any order, and the result is identical on every target.

use rayon::prelude::*;

use crate::algo::fixed::DIFFUSE_WHITE;

/// The share of a channel's samples at or below the measured value.
pub const PERCENTILE: f64 = 0.999;

/// A channel whose [`PERCENTILE`] sits below `2^-COLLAPSE_STOPS` of diffuse white has
/// collapsed. Tuned on every real frame in `../nc-assets` (`docs/progress/algo.md`).
pub const COLLAPSE_STOPS: i32 = 10;

/// Mantissa bits kept in a bin key: 8 bins per octave, each under 0.17 stop wide.
const MANTISSA_BITS: u32 = 3;
const SHIFT: u32 = 23 - MANTISSA_BITS;
/// Every positive finite f32 key, `0x7f7f_ffff >> SHIFT`, fits below this.
const BINS: usize = (0x7f80_0000_u32 >> SHIFT) as usize;
/// Samples per parallel band: a multiple of 3, so each band starts on red.
const BAND_SAMPLES: usize = 3 << 18;

/// Each channel's [`PERCENTILE`] of a display-referred buffer, as the lower edge of the
/// histogram bin it falls in: within 0.17 stop below the true value, and 0 when the
/// percentile is at or below 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrightEnd(pub [f32; 3]);

impl BrightEnd {
    /// The channels whose bright end is below the collapse threshold.
    pub fn collapsed(&self) -> Vec<usize> {
        let threshold = DIFFUSE_WHITE * (-COLLAPSE_STOPS as f32).exp2();
        (0..3).filter(|&c| self.0[c] < threshold).collect()
    }

    /// Channel `c`'s bright end in stops relative to diffuse white; `-inf` at 0.
    pub fn stops(&self, c: usize) -> f32 {
        (self.0[c] / DIFFUSE_WHITE).log2()
    }
}

/// The histogram key of a sample: 0 for anything not positive and finite.
fn key(v: f32) -> usize {
    if v > 0.0 && v.is_finite() {
        (v.to_bits() >> SHIFT) as usize
    } else {
        0
    }
}

/// Measure interleaved RGB `rgb`. An empty buffer measures 0 on every channel.
pub fn measure(rgb: &[f32]) -> BrightEnd {
    let counts = rgb
        .par_chunks(BAND_SAMPLES)
        .fold(
            || vec![0u64; 3 * BINS],
            |mut counts, band| {
                for (i, &v) in band.iter().enumerate() {
                    counts[(i % 3) * BINS + key(v)] += 1;
                }
                counts
            },
        )
        .reduce(
            || vec![0u64; 3 * BINS],
            |mut a, b| {
                a.iter_mut().zip(&b).for_each(|(a, b)| *a += b);
                a
            },
        );
    BrightEnd(std::array::from_fn(|c| {
        percentile_edge(&counts[c * BINS..(c + 1) * BINS])
    }))
}

/// The lower edge of the bin holding the [`PERCENTILE`] sample of `bins`.
fn percentile_edge(bins: &[u64]) -> f32 {
    let n: u64 = bins.iter().sum();
    if n == 0 {
        return 0.0;
    }
    let rank = ((n as f64 * PERCENTILE).ceil() as u64).max(1);
    let mut seen = 0;
    for (k, &count) in bins.iter().enumerate() {
        seen += count;
        if seen >= rank {
            return if k == 0 {
                0.0
            } else {
                f32::from_bits((k as u32) << SHIFT)
            };
        }
    }
    unreachable!("the rank is at most the sample count")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` pixels of `rgb`.
    fn flat(rgb: [f32; 3], n: usize) -> Vec<f32> {
        rgb.iter().copied().cycle().take(3 * n).collect()
    }

    #[test]
    fn a_bin_edge_is_exact_and_a_value_reads_at_most_a_bin_low() {
        let m = measure(&flat([1.0, 0.25, 2f32.powi(-12)], 10));
        assert_eq!(m.0, [1.0, 0.25, 2f32.powi(-12)]);
        let v = 0.3f32;
        let edge = measure(&flat([v; 3], 1)).0[0];
        assert!(edge <= v && v / edge < 1.125, "{edge} for {v}");
    }

    #[test]
    fn the_percentile_ignores_the_brightest_tenth_of_a_percent() {
        // 1000 pixels: one glint at white leaves the 999th sample where the rest are.
        let mut rgb = flat([2f32.powi(-14); 3], 999);
        rgb.extend([1.0; 3]);
        let m = measure(&rgb);
        assert_eq!(m.0, [2f32.powi(-14); 3]);
        assert_eq!(m.collapsed(), [0, 1, 2]);
        // Two glints are more than 0.1 %: the percentile reaches them.
        rgb.extend([1.0; 3]);
        assert_eq!(measure(&rgb).collapsed(), Vec::<usize>::new());
    }

    #[test]
    fn the_threshold_splits_per_channel_at_its_power_of_two() {
        let at = DIFFUSE_WHITE * 2f32.powi(-COLLAPSE_STOPS);
        let below = f32::from_bits(at.to_bits() - 1);
        let m = measure(&flat([at, below, 0.5], 4));
        assert_eq!(m.collapsed(), [1]);
        assert_eq!(m.stops(0), -COLLAPSE_STOPS as f32);
    }

    #[test]
    fn zero_negative_and_non_finite_samples_read_as_zero() {
        for v in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY] {
            let m = measure(&flat([v, 1.0, 1.0], 3));
            assert_eq!(m.0[0], 0.0, "{v}");
            assert_eq!(m.stops(0), f32::NEG_INFINITY);
        }
        assert_eq!(measure(&[]).0, [0.0; 3]);
    }

    #[test]
    fn bands_fold_to_the_sequential_answer() {
        // Spans several bands, with a ramp so every bin boundary is exercised.
        let n = BAND_SAMPLES; // pixels, so three bands
        let rgb: Vec<f32> = (0..3 * n)
            .map(|i| (i as f32 / (3 * n) as f32) * 2f32.powi(-((i % 3) as i32) * 7))
            .collect();
        let mut counts = vec![0u64; 3 * BINS];
        for (i, &v) in rgb.iter().enumerate() {
            counts[(i % 3) * BINS + key(v)] += 1;
        }
        let sequential: [f32; 3] =
            std::array::from_fn(|c| percentile_edge(&counts[c * BINS..(c + 1) * BINS]));
        assert_eq!(measure(&rgb).0, sequential);
    }

    /// The threshold's evidence: each frame's bright end, from the first page of every
    /// `--export-pre-encode` TIFF under `NC_NEAR_BLACK_DIR` (an SDR render's
    /// `sdr-linear`). Prints derived numbers only, darkest frame first. Export each frame
    /// with `hanten convert <frame> --params <roll recipe> -o … --export-pre-encode
    /// <dir>/<frame>.tif`, then
    /// `NC_NEAR_BLACK_DIR=<dir> cargo test --release near_black_probe -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs NC_NEAR_BLACK_DIR of pre-encode exports; run with --ignored --nocapture"]
    fn near_black_probe() {
        use tiff::decoder::{Decoder, DecodingResult};
        let Ok(dir) = std::env::var("NC_NEAR_BLACK_DIR") else {
            eprintln!("SKIP: set NC_NEAR_BLACK_DIR");
            return;
        };
        let mut paths: Vec<_> = walk(std::path::Path::new(&dir));
        paths.sort();
        let mut rows: Vec<(f32, String, [f32; 3])> = paths
            .iter()
            .filter_map(|path| {
                let file = std::fs::File::open(path).unwrap();
                let mut decoder = Decoder::new(std::io::BufReader::new(file)).unwrap();
                let DecodingResult::F32(rgb) = decoder.read_image().unwrap() else {
                    eprintln!("skip {}: not an f32 page", path.display());
                    return None;
                };
                let m = measure(&rgb);
                let stops = std::array::from_fn(|c| m.stops(c));
                let name = path.strip_prefix(&dir).unwrap().display().to_string();
                Some((stops.into_iter().fold(f32::INFINITY, f32::min), name, stops))
            })
            .collect();
        rows.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        println!(
            "p{:.1} per channel, stops below diffuse white",
            PERCENTILE * 100.0
        );
        for (_, name, [r, g, b]) in &rows {
            println!("{r:7.2} {g:7.2} {b:7.2}  {name}");
        }
        println!("{} frames; collapse below -{COLLAPSE_STOPS}", rows.len());
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .flat_map(|e| {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p)
                } else if p.extension().is_some_and(|x| x == "tif") {
                    vec![p]
                } else {
                    vec![]
                }
            })
            .collect()
    }
}
