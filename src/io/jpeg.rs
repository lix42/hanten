//! Baseline JPEG encoding shared by both gain-map containers: the current chain's
//! Ultra HDR package (`io::ultra_hdr`) and the new chain's ISO-only one
//! (`io::iso_gain_map`).

use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use rayon::prelude::*;

use crate::io::QUANTIZE_BAND_SAMPLES;
use crate::types::{EncodeReport, NcError, OutputStats, Result};

const JPEG_QUALITY: u8 = 95;

/// Encode 8-bit samples as a baseline 4:4:4 JPEG at [`JPEG_QUALITY`], with an
/// optional ICC profile and APP2 segments (each given as its content after the
/// length field), written after JFIF and the ICC chunks in the order given — every
/// one inside the header block, ahead of the frame header.
pub(crate) fn encode_jpeg(
    rgb: &[u8],
    width: u32,
    height: u32,
    icc: Option<&[u8]>,
    label: &str,
    color_type: ColorType,
    app2: Vec<Vec<u8>>,
) -> Result<Vec<u8>> {
    let width = u16::try_from(width).map_err(|_| {
        NcError::Unsupported(format!(
            "{label} width exceeds the JPEG limit (got {width})"
        ))
    })?;
    let height = u16::try_from(height).map_err(|_| {
        NcError::Unsupported(format!(
            "{label} height exceeds the JPEG limit (got {height})"
        ))
    })?;
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, JPEG_QUALITY);
    encoder.set_sampling_factor(SamplingFactor::R_4_4_4);
    if let Some(profile) = icc {
        encoder
            .add_icc_profile(profile)
            .map_err(|e| NcError::Write(format!("embedding {label} ICC profile: {e}")))?;
    }
    for segment in app2 {
        encoder
            .add_app_segment(2, segment)
            .map_err(|e| NcError::Write(format!("embedding {label} APP2 segment: {e}")))?;
    }
    encoder
        .encode(rgb, width, height, color_type)
        .map_err(|e| NcError::Write(format!("encoding {label} JPEG: {e}")))?;
    Ok(bytes)
}

/// Quantize `[0, 1]` samples to 8 bits, counting every sample clamped or
/// non-finite, with the per-channel means of what was stored.
pub(crate) fn quantize_u8(rgb: &[f32]) -> (Vec<u8>, EncodeReport, OutputStats) {
    // One parallel pass over bands whose length is a multiple of 3
    // (`QUANTIZE_BAND_SAMPLES`), so a sample's channel is its index within the band
    // modulo 3. Loss counts and channel sums are integers, so their totals are exact
    // in any order.
    #[derive(Default)]
    struct Band {
        non_finite: u64,
        clipped_low: u64,
        clipped_high: u64,
        sums: [u64; 3],
    }
    let mut bytes = vec![0_u8; rgb.len()];
    let band = bytes
        .par_chunks_mut(QUANTIZE_BAND_SAMPLES)
        .zip(rgb.par_chunks(QUANTIZE_BAND_SAMPLES))
        .map(|(out, values)| {
            let mut band = Band::default();
            for (index, (o, value)) in out.iter_mut().zip(values.iter().copied()).enumerate() {
                let byte = if !value.is_finite() {
                    band.non_finite += 1;
                    0
                } else if value < 0.0 {
                    band.clipped_low += 1;
                    0
                } else if value > 1.0 {
                    band.clipped_high += 1;
                    u8::MAX
                } else {
                    (value * u8::MAX as f32).round() as u8
                };
                band.sums[index % 3] += u64::from(byte);
                *o = byte;
            }
            band
        })
        .reduce(Band::default, |a, b| Band {
            non_finite: a.non_finite + b.non_finite,
            clipped_low: a.clipped_low + b.clipped_low,
            clipped_high: a.clipped_high + b.clipped_high,
            sums: [
                a.sums[0] + b.sums[0],
                a.sums[1] + b.sums[1],
                a.sums[2] + b.sums[2],
            ],
        });
    let loss = EncodeReport {
        total_samples: rgb.len() as u64,
        clipped_low: band.clipped_low,
        clipped_high: band.clipped_high,
        non_finite: band.non_finite,
    };
    let sums = band.sums;
    // Whole triples, as the sequential loop counted them (its `index % 3 == 2`).
    let pixels = (rgb.len() / 3) as u64;
    let mean = if pixels == 0 {
        [0.0; 3]
    } else {
        [
            sums[0] as f64 / pixels as f64 / u8::MAX as f64,
            sums[1] as f64 / pixels as f64 / u8::MAX as f64,
            sums[2] as f64 / pixels as f64 / u8::MAX as f64,
        ]
    };
    (bytes, loss, OutputStats { mean })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantization_counts_every_loss() {
        let (_, loss, stats) = quantize_u8(&[-1.0, 0.5, f32::NAN, 2.0, 1.0, 0.0]);
        assert_eq!(loss.total_samples, 6);
        assert_eq!(loss.clipped_low, 1);
        assert_eq!(loss.clipped_high, 1);
        assert_eq!(loss.non_finite, 1);
        assert_eq!(stats.mean[0], 0.5);
    }

    #[test]
    fn quantize_u8_matches_one_sequential_pass_across_band_boundaries() {
        // Spans two full `QUANTIZE_BAND_SAMPLES` bands plus a ragged triple, so a
        // band that misaligned the `% 3` channel sums would show up here; every
        // other test of this function is six samples inside a single band.
        let len = QUANTIZE_BAND_SAMPLES * 2 + 6;
        let rgb: Vec<f32> = (0..len)
            .map(|i| match i % 1777 {
                0 => f32::NAN,
                1 => -0.5,
                2 => 1.25,
                n => n as f32 / 1776.0,
            })
            .collect();

        let (bytes, loss, stats) = quantize_u8(&rgb);

        let mut want = Vec::with_capacity(len);
        let (mut non_finite, mut low, mut high) = (0_u64, 0_u64, 0_u64);
        let mut sums = [0_u64; 3];
        for (i, &v) in rgb.iter().enumerate() {
            let byte = if !v.is_finite() {
                non_finite += 1;
                0
            } else if v < 0.0 {
                low += 1;
                0
            } else if v > 1.0 {
                high += 1;
                u8::MAX
            } else {
                (v * u8::MAX as f32).round() as u8
            };
            sums[i % 3] += u64::from(byte);
            want.push(byte);
        }

        assert_eq!(bytes, want);
        assert_eq!(loss.non_finite, non_finite);
        assert_eq!(loss.clipped_low, low);
        assert_eq!(loss.clipped_high, high);
        let pixels = (len / 3) as f64;
        for (c, (sum, mean)) in sums.iter().zip(&stats.mean).enumerate() {
            let expect = *sum as f64 / pixels / u8::MAX as f64;
            assert_eq!(*mean, expect, "channel {c}");
        }
    }
}
