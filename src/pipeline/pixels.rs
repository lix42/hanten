//! Parallel drivers for the per-pixel **map** stages between reconstruction and
//! encode.
//!
//! Each stage keeps its own per-pixel kernel; this module owns only the loop that
//! applies it across interleaved RGB buffers on every core. Two rules, decided in
//! `docs/gpu-rendering-spike.md`:
//!
//! - A driver here is a pure map, so its output is byte-identical to the sequential
//!   loop it replaced. Stage errors quote the offending pixel's index, so which
//!   failure is reported must not depend on thread scheduling: the fallible drivers
//!   use rayon's positional `find_map_first`, which yields the error at the
//!   **lowest** index — the one the sequential loop produced — and stops the work
//!   to its right.
//! - There is no floating-point reduction here. A sum must run in a **fixed
//!   order** to be reproducible; today `hdr::render_linear`'s MaxFALL sum is one
//!   sequential pass over the mapped buffer. Fixed index bands with a sequential
//!   combine would also be reproducible, but they associate the sum differently
//!   and so can move a rounded nit value — a byte change to `clli`, not a
//!   refactor. Integer counts, `min` and `max` are exact and may be folded in
//!   parallel wherever they occur.
//!
//! `rgb.len() % 3 == 0` is a `LinearImage` invariant. The infallible driver asserts
//! it in debug builds, as the sequential loops it replaced did; the fallible ones
//! already return `Result`, so they report a ragged buffer instead of skipping its
//! tail.

use rayon::prelude::*;

use crate::types::{NcError, Result};

/// Apply `f` to every RGB triple of `rgb` in place, in parallel.
pub fn map_in_place(rgb: &mut [f32], f: impl Fn(&mut [f32; 3]) + Send + Sync) {
    let (pixels, rest) = rgb.as_chunks_mut::<3>();
    debug_assert!(rest.is_empty(), "rgb length must be a multiple of 3");
    pixels.par_iter_mut().for_each(f);
}

/// Map every RGB triple of `src` through the fallible `f(index, pixel)` into a new
/// buffer, in parallel, returning the lowest-indexed error if any pixel fails.
pub fn try_map(
    src: &[f32],
    f: impl Fn(usize, [f32; 3]) -> Result<[f32; 3]> + Send + Sync,
) -> Result<Vec<f32>> {
    let pixels = triples(src)?;
    let mut out = vec![0.0_f32; src.len()];
    let (out_pixels, _) = out.as_chunks_mut::<3>();
    let first_error = out_pixels
        .par_iter_mut()
        .zip(pixels.par_iter())
        .enumerate()
        .find_map_first(|(index, (dst, px))| match f(index, *px) {
            Ok(rendered) => {
                *dst = rendered;
                None
            }
            Err(error) => Some(error),
        });
    first_error.map_or(Ok(out), Err)
}

/// In-place counterpart of [`try_map`]: `f(index, pixel)` rewrites the triple or
/// fails. On failure the buffer is partially rewritten and must be discarded.
pub fn try_map_in_place(
    rgb: &mut [f32],
    f: impl Fn(usize, &mut [f32; 3]) -> Result<()> + Send + Sync,
) -> Result<()> {
    let pixels = triples_mut(rgb)?;
    let first_error = pixels
        .par_iter_mut()
        .enumerate()
        .find_map_first(|(index, px)| f(index, px).err());
    first_error.map_or(Ok(()), Err)
}

/// Two-input, two-output counterpart of [`try_map`] for a kernel that reads a pixel
/// from each of two same-sized buffers and writes one to each of two new buffers.
pub fn try_zip_map(
    a: &[f32],
    b: &[f32],
    f: impl Fn(usize, [f32; 3], [f32; 3]) -> Result<([f32; 3], [f32; 3])> + Send + Sync,
) -> Result<(Vec<f32>, Vec<f32>)> {
    let (pixels_a, pixels_b) = (triples(a)?, triples(b)?);
    if pixels_a.len() != pixels_b.len() {
        return Err(NcError::Other(format!(
            "rgb buffers differ in length: {} vs {} samples",
            a.len(),
            b.len()
        )));
    }
    let mut out_a = vec![0.0_f32; a.len()];
    let mut out_b = vec![0.0_f32; b.len()];
    let first_error = out_a
        .as_chunks_mut::<3>()
        .0
        .par_iter_mut()
        .zip(out_b.as_chunks_mut::<3>().0.par_iter_mut())
        .zip(pixels_a.par_iter().zip(pixels_b.par_iter()))
        .enumerate()
        .find_map_first(
            |(index, ((dst_a, dst_b), (px_a, px_b)))| match f(index, *px_a, *px_b) {
                Ok((ra, rb)) => {
                    *dst_a = ra;
                    *dst_b = rb;
                    None
                }
                Err(error) => Some(error),
            },
        );
    first_error.map_or(Ok((out_a, out_b)), Err)
}

/// View an interleaved RGB buffer as triples, refusing a ragged one.
pub(crate) fn triples(rgb: &[f32]) -> Result<&[[f32; 3]]> {
    let (pixels, rest) = rgb.as_chunks::<3>();
    if !rest.is_empty() {
        return Err(ragged(rgb.len()));
    }
    Ok(pixels)
}

/// Mutable counterpart of [`triples`].
pub(crate) fn triples_mut(rgb: &mut [f32]) -> Result<&mut [[f32; 3]]> {
    let len = rgb.len();
    let (pixels, rest) = rgb.as_chunks_mut::<3>();
    if !rest.is_empty() {
        return Err(ragged(len));
    }
    Ok(pixels)
}

fn ragged(len: usize) -> NcError {
    NcError::Other(format!("rgb buffer length {len} is not a multiple of 3"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(pixels: usize) -> Vec<f32> {
        (0..pixels * 3).map(|i| i as f32 * 0.25).collect()
    }

    #[test]
    fn map_in_place_matches_the_sequential_loop() {
        let mut parallel = ramp(10_007);
        let mut sequential = parallel.clone();
        let kernel = |px: &mut [f32; 3]| {
            let (r, g, b) = (px[0], px[1], px[2]);
            px[0] = r * 0.5 + g;
            px[1] = g - b * 0.125;
            px[2] = b + r;
        };
        map_in_place(&mut parallel, kernel);
        for px in sequential.as_chunks_mut::<3>().0 {
            kernel(px);
        }
        assert_eq!(parallel, sequential);
    }

    #[test]
    fn try_map_matches_the_sequential_loop_and_keeps_order() {
        let src = ramp(10_007);
        let out = try_map(&src, |index, px| Ok([px[0] + index as f32, px[1], px[2]])).unwrap();
        for (index, (o, s)) in out
            .as_chunks::<3>()
            .0
            .iter()
            .zip(src.as_chunks::<3>().0)
            .enumerate()
        {
            assert_eq!(*o, [s[0] + index as f32, s[1], s[2]]);
        }
    }

    #[test]
    fn try_map_in_place_matches_try_map() {
        let src = ramp(10_007);
        let expect = try_map(&src, |index, px| Ok([px[0] + index as f32, px[1], px[2]])).unwrap();
        let mut got = src;
        try_map_in_place(&mut got, |index, px| {
            px[0] += index as f32;
            Ok(())
        })
        .unwrap();
        assert_eq!(got, expect);
    }

    #[test]
    fn try_zip_map_matches_the_sequential_loop_on_both_outputs() {
        let a = ramp(10_007);
        let b: Vec<f32> = a.iter().map(|v| v * 2.0).collect();
        let (out_a, out_b) = try_zip_map(&a, &b, |index, pa, pb| {
            Ok((
                [pa[0] + pb[0], pa[1], index as f32],
                [pb[2] - pa[2], 0.0, 1.0],
            ))
        })
        .unwrap();
        let (pa, pb) = (a.as_chunks::<3>().0, b.as_chunks::<3>().0);
        for (index, (oa, ob)) in out_a
            .as_chunks::<3>()
            .0
            .iter()
            .zip(out_b.as_chunks::<3>().0)
            .enumerate()
        {
            assert_eq!(
                *oa,
                [pa[index][0] + pb[index][0], pa[index][1], index as f32]
            );
            assert_eq!(*ob, [pb[index][2] - pa[index][2], 0.0, 1.0]);
        }
    }

    #[test]
    fn fallible_drivers_report_the_lowest_failing_index() {
        // Failures at 249, 499, 749, 999: a parallel scheduler may reach any of them
        // first; every driver must still report 249 every time.
        let src = ramp(1_000);
        let fail = |index: usize| index % 250 == 249;
        for _ in 0..8 {
            let err = try_map(&src, |index, px| {
                if fail(index) {
                    Err(NcError::Other(format!("bad pixel {index}")))
                } else {
                    Ok(px)
                }
            })
            .unwrap_err();
            assert!(err.to_string().contains("bad pixel 249"), "{err}");

            let mut buffer = src.clone();
            let err = try_map_in_place(&mut buffer, |index, _| {
                if fail(index) {
                    Err(NcError::Other(format!("bad pixel {index}")))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
            assert!(err.to_string().contains("bad pixel 249"), "{err}");

            let err = try_zip_map(&src, &src, |index, pa, pb| {
                if fail(index) {
                    Err(NcError::Other(format!("bad pixel {index}")))
                } else {
                    Ok((pa, pb))
                }
            })
            .unwrap_err();
            assert!(err.to_string().contains("bad pixel 249"), "{err}");
        }
    }

    #[test]
    fn fallible_drivers_reject_a_ragged_or_mismatched_buffer() {
        let err = try_map(&[0.0, 1.0, 2.0, 3.0], |_, px| Ok(px)).unwrap_err();
        assert!(err.to_string().contains("not a multiple of 3"), "{err}");
        let err = try_map_in_place(&mut [0.0, 1.0], |_, _| Ok(())).unwrap_err();
        assert!(err.to_string().contains("not a multiple of 3"), "{err}");
        let err = try_zip_map(&[0.0; 6], &[0.0; 3], |_, a, b| Ok((a, b))).unwrap_err();
        assert!(err.to_string().contains("differ in length"), "{err}");
    }
}
