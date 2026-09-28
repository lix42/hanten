//! The gain-map image: per-channel gains quantized into a
//! half-resolution, 8-bit, three-channel map, and the window its metadata states.
//!
//! Each channel is normalized over **its own** `log2` window — the gains' exact
//! per-channel extent, read from [`GainRange`] — so a channel with little gain keeps
//! its full code range. The ISO 21496-1 dialect states one window per channel
//! (`is_multichannel`); the Ultra HDR v1 XMP cannot, which is why this
//! destination is ISO-only. Encoding gamma is `1`: a code is linear in `log2` gain.
//!
//! **Half resolution, centre-aligned** (user decision, 2026-09-27). Each output
//! sample is the bilinear blend of the four nearest *normalized* gains, i.e. the
//! blend is taken in the `log2` domain, as a decoder's upsampling interpolates the
//! stored codes. ISO 21496-1 6.2.2 NOTE 1 prefers a co-sited phase; the NOTE is
//! informative, and the removed chain's Ultra HDR map was centre-aligned too.
//!
//! **A spatially constant channel states `min == max`** (5.2.5.3 allows equality) and
//! writes code `0`, which decodes to that gain exactly. **A flat map** ([`GainRange::flat`],
//! every gain within rounding of `1`) is written as exactly that: all zeros with a
//! window of `0` on every channel, so the file declares no gain rather than spreading
//! rounding noise over the full code range. The caller reports it; it is never widened
//! to look live.
//!
//! Written fresh, per the migration rule, rather than from the removed chain's
//! `pipeline::gain_map`.

use rayon::prelude::*;
use serde::Serialize;

use crate::pipeline::gain_ratio::{GainRange, GainRatios};
use crate::pipeline::pixels;
use crate::types::Result;

/// The offset `o` added to both renditions before the gain is taken — format policy,
/// not a conversion knob. `1/64` is the Ultra HDR v1 value the removed chain's maps
/// stated; it keeps a black base's gain finite without lifting its shadows visibly.
pub const OFFSET: f32 = 1.0 / 64.0;

/// The encoding gamma every channel states: a code is linear in `log2` gain.
pub const GAMMA: f32 = 1.0;

/// The largest 8-bit code, as a float.
const MAX_CODE: f32 = u8::MAX as f32;

/// The per-channel `log2` gain window a map's codes span: code `0` is `log2_min`,
/// code `255` is `log2_max`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct GainWindow {
    pub log2_min: [f32; 3],
    pub log2_max: [f32; 3],
}

impl GainWindow {
    /// The `log2` gain a code decodes to on `channel` — the decoder's side of
    /// [`encode`], at [`GAMMA`]. Test-only: nc never decodes its own maps.
    #[cfg(test)]
    pub fn decode(&self, channel: usize, code: u8) -> f32 {
        let span = self.log2_max[channel] - self.log2_min[channel];
        self.log2_min[channel] + f32::from(code) / MAX_CODE * span
    }
}

/// A quantized gain map: interleaved `r,g,b` codes and the window they span.
#[derive(Debug)]
pub struct GainMapImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub window: GainWindow,
    /// The offset added to both renditions, as [`GainRatios::offset`] took it.
    pub offset: f32,
}

/// Quantize `ratios` into a half-resolution, three-channel, 8-bit map.
///
/// The gains are finite and positive by [`crate::pipeline::gain_ratio::between`]'s
/// contract, so every `log2` here is finite.
pub fn encode(ratios: &GainRatios) -> Result<GainMapImage> {
    let (width, height) = (ratios.width(), ratios.height());
    let GainRange { min, max, flat } = ratios.range();
    let window = if flat {
        GainWindow {
            log2_min: [0.0; 3],
            log2_max: [0.0; 3],
        }
    } else {
        GainWindow {
            log2_min: min.map(f32::log2),
            log2_max: max.map(f32::log2),
        }
    };
    let span: [f32; 3] = std::array::from_fn(|c| window.log2_max[c] - window.log2_min[c]);
    let gains = pixels::triples(ratios.rgb())?;
    let normalized = |x: u32, y: u32, c: usize| -> f32 {
        if span[c] == 0.0 {
            return 0.0;
        }
        let gain = gains[(y * width + x) as usize][c];
        ((gain.log2() - window.log2_min[c]) / span[c]).clamp(0.0, 1.0)
    };

    let (out_width, out_height) = (width.div_ceil(2), height.div_ceil(2));
    let mut rgb = vec![0_u8; out_width as usize * out_height as usize * 3];
    // A resample, not a per-pixel map, so it does not go through `pipeline::pixels`;
    // it keeps that module's rule anyway: each output sample depends only on its own
    // taps and nothing is reduced, so rows fill in any order and the bytes do not
    // depend on scheduling.
    rgb.par_chunks_mut((out_width as usize * 3).max(1))
        .enumerate()
        .for_each(|(y, row)| {
            let (y0, y1, ty) = taps(y as u32, out_height, height);
            for x in 0..out_width {
                let (x0, x1, tx) = taps(x, out_width, width);
                for c in 0..3 {
                    let top = lerp(normalized(x0, y0, c), normalized(x1, y0, c), tx);
                    let bottom = lerp(normalized(x0, y1, c), normalized(x1, y1, c), tx);
                    row[x as usize * 3 + c] = (lerp(top, bottom, ty) * MAX_CODE).round() as u8;
                }
            }
        });

    Ok(GainMapImage {
        width: out_width,
        height: out_height,
        rgb,
        window,
        offset: ratios.offset(),
    })
}

/// The two source samples an output sample blends along one axis, and the weight of
/// the second — centre-aligned: output centre `i + 0.5` maps to source centre
/// `(i + 0.5) · src / dst`.
fn taps(index: u32, dst_len: u32, src_len: u32) -> (u32, u32, f32) {
    let coordinate = (((f64::from(index) + 0.5) * f64::from(src_len) / f64::from(dst_len)) - 0.5)
        .clamp(0.0, f64::from(src_len - 1));
    let lower = coordinate.floor() as u32;
    let upper = (lower + 1).min(src_len - 1);
    (lower, upper, (coordinate - f64::from(lower)) as f32)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::gain_ratio::between;
    use crate::types::LinearImage;

    fn image(width: u32, height: u32, rgb: Vec<f32>) -> LinearImage {
        LinearImage::new(width, height, rgb, None).unwrap()
    }

    /// Gains for a `width × height` frame whose pixel `i` has the base `sdr(i)` and
    /// the alternate `hdr(i)`.
    fn ratios(
        width: u32,
        height: u32,
        pixel: impl Fn(usize) -> ([f32; 3], [f32; 3]),
    ) -> GainRatios {
        let pixels: Vec<_> = (0..(width * height) as usize).map(pixel).collect();
        let sdr = pixels.iter().flat_map(|(s, _)| *s).collect();
        let hdr = pixels.iter().flat_map(|(_, h)| *h).collect();
        between(
            &image(width, height, sdr),
            &image(width, height, hdr),
            OFFSET,
        )
        .unwrap()
    }

    #[test]
    fn the_map_is_half_resolution_rounding_up() {
        for ((width, height), expected) in [
            ((4, 4), (2, 2)),
            ((5, 3), (3, 2)),
            ((1, 1), (1, 1)),
            ((2, 7), (1, 4)),
        ] {
            let map = encode(&ratios(width, height, |_| ([0.5; 3], [0.5; 3]))).unwrap();
            assert_eq!((map.width, map.height), expected, "{width}×{height}");
            assert_eq!(map.rgb.len(), (expected.0 * expected.1 * 3) as usize);
        }
    }

    #[test]
    fn a_flat_map_is_all_zeros_with_a_zero_window() {
        let map = encode(&ratios(3, 3, |i| {
            ([i as f32 / 9.0; 3], [i as f32 / 9.0; 3])
        }))
        .unwrap();
        assert!(map.rgb.iter().all(|code| *code == 0), "{:?}", map.rgb);
        assert_eq!(
            map.window,
            GainWindow {
                log2_min: [0.0; 3],
                log2_max: [0.0; 3],
            }
        );
        // Code 0 decodes to exactly the constant gain.
        assert_eq!(map.window.decode(0, 0), 0.0);
    }

    #[test]
    fn a_map_of_rounding_noise_is_written_flat() {
        // Gains a float step off 1: flat within tolerance, so the file states a zero
        // window and all zeros rather than stretching the noise over 0–255.
        let gains = ratios(2, 2, |i| {
            let hdr = if i == 0 { 0.500_000_1 } else { 0.5 };
            ([0.5; 3], [hdr, 0.5, 0.5])
        });
        assert!(gains.range().max[0] > 1.0 && gains.range().flat);
        let map = encode(&gains).unwrap();
        assert!(map.rgb.iter().all(|code| *code == 0), "{:?}", map.rgb);
        assert_eq!(map.window.log2_max, [0.0; 3]);
    }

    #[test]
    fn the_window_ends_are_the_first_and_last_codes() {
        // 4×2: the left 2×2 block is identical in both renditions, the right block
        // is brighter on red only. Each output sample averages one whole block.
        let map = encode(&ratios(4, 2, |i| {
            if i % 4 < 2 {
                ([0.5; 3], [0.5; 3])
            } else {
                ([1.0, 0.5, 0.5], [3.0, 0.5, 0.5])
            }
        }))
        .unwrap();
        assert_eq!(map.rgb, vec![0, 0, 0, 255, 0, 0]);
        let red = ((3.0 + OFFSET) / (1.0 + OFFSET)).log2();
        assert_eq!(map.window.log2_min, [0.0; 3]);
        assert!(
            (map.window.log2_max[0] - red).abs() < 1e-6,
            "{:?}",
            map.window
        );
        assert_eq!(map.window.log2_max[1..], [0.0, 0.0]);
    }

    #[test]
    fn each_channel_spans_its_own_window() {
        // Red gains a stop, blue loses one: two windows, neither shared.
        let map = encode(&ratios(2, 2, |i| {
            if i == 0 {
                ([0.5; 3], [0.5; 3])
            } else {
                ([0.5; 3], [1.0 + OFFSET, 0.5, 0.25 - OFFSET / 2.0])
            }
        }))
        .unwrap();
        let [r, g, b] = [0, 1, 2].map(|c| (map.window.log2_min[c], map.window.log2_max[c]));
        assert!(r.0 == 0.0 && (r.1 - 1.0).abs() < 1e-6, "{r:?}");
        assert_eq!(g, (0.0, 0.0));
        assert!((b.0 + 1.0).abs() < 1e-6 && b.1 == 0.0, "{b:?}");
    }

    #[test]
    fn codes_decode_to_the_blended_log_gain_within_half_a_code() {
        // A 4×4 frame of 2×2 blocks, so every output sample is one block's gain.
        let levels = [
            [1.0, 1.0, 1.0],
            [2.0, 1.2, 0.5],
            [4.0, 0.8, 0.9],
            [1.5, 3.0, 2.0],
        ];
        let block = |i: usize| (i % 4) / 2 + 2 * ((i / 4) / 2);
        let gains = ratios(4, 4, |i| {
            let level = levels[block(i)];
            // hdr = gain · (sdr + o) − o over a base of 0.25.
            ([0.25; 3], level.map(|g| g * (0.25 + OFFSET) - OFFSET))
        });
        let map = encode(&gains).unwrap();
        for (pixel, level) in levels.iter().enumerate() {
            for (c, gain) in level.iter().enumerate() {
                let decoded = map.window.decode(c, map.rgb[pixel * 3 + c]);
                let span = map.window.log2_max[c] - map.window.log2_min[c];
                let error = (decoded - gain.log2()).abs();
                assert!(
                    error <= span / MAX_CODE / 2.0 + 1e-5,
                    "pixel {pixel} channel {c}: {decoded} vs {}",
                    gain.log2()
                );
            }
        }
    }

    #[test]
    fn encoding_is_deterministic() {
        let gains = ratios(7, 5, |i| {
            let v = (i as f32 * 0.37).fract();
            ([v; 3], [v * 1.5, v, v * 0.7])
        });
        let (a, b) = (encode(&gains).unwrap(), encode(&gains).unwrap());
        assert_eq!((a.rgb, a.window), (b.rgb, b.window));
    }
}
