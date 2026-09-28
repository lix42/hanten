//! The gain-map JPEG destination: an SDR base and a three-channel gain map, described
//! by ISO 21496-1 metadata **only**, in a Multi-Picture Format container nc writes
//! itself (`nf-destinations/gain-map-destination`).
//!
//! No libultrahdr and no Ultra HDR v1 XMP: that dialect cannot describe a
//! per-channel map, and libultrahdr's packager always writes it. The file is
//!
//! ```text
//! base:     SOI · APP0 JFIF · APP2 ICC · APP2 ISO version (C.4.3) · APP2 MPF · DQT · SOF0 …  EOI
//! gain map: SOI · APP0 JFIF · APP2 ISO metadata (C.4.6) · DQT · SOF0 …  EOI
//! ```
//!
//! **Every APP segment sits before `SOF0`.** A reader stops scanning for `APPn` at the
//! frame header; the removed Ultra HDR container once shipped an ISO segment past it
//! and ImageIO saw no gain map at all. The JPEG encoder writes every segment it is given in the header block, in order, so
//! nothing here is spliced after encoding.
//!
//! **MPF** follows libultrahdr's layout — big-endian, an MP Index IFD of three tags
//! (version, image count, entries) and no attribute IFD — which is the one ImageIO has
//! been observed to read, with one deliberate change: the gain map's MP Type Code is
//! `050000`, the code CIPA DC-007 Table 4 assigns an ISO 21496-1 gain map, where
//! libultrahdr writes `000000` ("shall not be used" in a Baseline MP File;
//! `output/mp-container-conformance`). The segment is written first as a same-sized
//! placeholder and filled in once the base's length is known, so no offset moves.
//! An image's offset is measured from the byte after the `MPF\0` label; the primary's
//! is `0`.
//!
//! No Exif: with none present and an ICC profile embedded, the ICC governs the base's
//! colour space (C.4.4). Whoever adds Exif must write `ColorSpace = Uncalibrated`,
//! never `1`, which forces an sRGB reading of the Display P3 base.
//!
//! Verify any change here with `scripts/iso-decoder-oracle/` (manual, macOS-only):
//! a well-formed file can still be one no decoder parses.

pub(crate) mod metadata;

use std::path::Path;

use jpeg_encoder::ColorType;

use crate::io::jpeg::{encode_jpeg, quantize_u8};
use crate::io::staged::{self, Staged};
use crate::pipeline::gain_encode::{self, GainMapImage};
use crate::types::{EncodeOutcome, LinearImage, NcError, Result};

/// The label an MPF APP2 segment's content starts with.
const MPF_LABEL: &[u8; 4] = b"MPF\0";
/// DC-007's MP Type Code for a Baseline MP Primary Image.
const MP_TYPE_BASELINE_PRIMARY: u32 = 0x03_0000;
/// DC-007 Table 4's MP Type Code for an ISO 21496-1 gain map.
const MP_TYPE_GAIN_MAP: u32 = 0x05_0000;
/// Where the MP entries start, from the TIFF header: the header (8), the tag count
/// (2), three 12-byte tags, and the next-IFD offset (4).
const MP_ENTRIES_OFFSET: u32 = 8 + 2 + 3 * 12 + 4;
const TIFF_UNDEFINED: u16 = 7;
const TIFF_LONG: u16 = 4;

/// Encode `base` (display-encoded, in the primaries `icc` names) and `map` into an
/// ISO-only gain-map JPEG staged at `path`.
///
/// `alternate_headroom` is the HDR rendition's peak over reference white, linear —
/// the headroom the metadata declares.
pub fn encode(
    base: &LinearImage,
    icc: &[u8],
    map: &GainMapImage,
    alternate_headroom: f32,
    path: &Path,
) -> Result<(Staged, EncodeOutcome)> {
    let fields = fields(map, alternate_headroom)?;
    let (base_rgb, loss, stats) = quantize_u8(&base.rgb);
    let bytes = assemble(&base_rgb, base.width, base.height, icc, map, &fields)?;
    Ok((
        staged::stage_bytes(path, &bytes)?,
        EncodeOutcome { loss, stats },
    ))
}

/// The ISO field set `map` states: its per-channel window, gamma and offset, over a
/// base at reference white.
fn fields(map: &GainMapImage, alternate_headroom: f32) -> Result<metadata::IsoGainMapFields> {
    metadata::fields(&metadata::Stated {
        gain_min_log2: map.window.log2_min,
        gain_max_log2: map.window.log2_max,
        gamma: [gain_encode::GAMMA; 3],
        base_offset: [map.offset; 3],
        alternate_offset: [map.offset; 3],
        alternate_headroom_linear: alternate_headroom,
    })
}

/// The finished file's bytes: the base, its MPF filled in, then the gain map.
fn assemble(
    base_rgb: &[u8],
    width: u32,
    height: u32,
    icc: &[u8],
    map: &GainMapImage,
    fields: &metadata::IsoGainMapFields,
) -> Result<Vec<u8>> {
    let gain_jpeg = encode_jpeg(
        &map.rgb,
        map.width,
        map.height,
        None,
        "gain map",
        ColorType::Rgb,
        vec![metadata::segment_content(&metadata::serialize_metadata(
            fields,
        )?)],
    )?;
    let mut base_jpeg = encode_jpeg(
        base_rgb,
        width,
        height,
        Some(icc),
        "SDR base",
        ColorType::Rgb,
        vec![
            metadata::segment_content(&metadata::serialize_version(fields)),
            mpf_content(0, 0, 0),
        ],
    )?;

    let label = header_app2(&base_jpeg, MPF_LABEL)?;
    let reference = label + MPF_LABEL.len();
    let too_large = || {
        NcError::Unsupported(format!(
            "the gain-map JPEG exceeds the 4 GiB a Multi-Picture Format offset can \
             address ({} + {} bytes)",
            base_jpeg.len(),
            gain_jpeg.len()
        ))
    };
    let primary_size = u32::try_from(base_jpeg.len()).map_err(|_| too_large())?;
    let gain_size = u32::try_from(gain_jpeg.len()).map_err(|_| too_large())?;
    let gain_offset = u32::try_from(base_jpeg.len() - reference).map_err(|_| too_large())?;
    gain_offset.checked_add(gain_size).ok_or_else(too_large)?;
    let mpf = mpf_content(primary_size, gain_size, gain_offset);
    base_jpeg[label..label + mpf.len()].copy_from_slice(&mpf);

    base_jpeg.extend_from_slice(&gain_jpeg);
    Ok(base_jpeg)
}

/// An MPF APP2 segment's content (after its length field) for a primary of
/// `primary_size` bytes and a gain map of `gain_size` bytes at `gain_offset`. The same
/// length whatever the values, so a placeholder can be overwritten in place.
fn mpf_content(primary_size: u32, gain_size: u32, gain_offset: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(MPF_LABEL.len() + MP_ENTRIES_OFFSET as usize + 32);
    bytes.extend_from_slice(MPF_LABEL);
    // Big-endian TIFF header; the MP Index IFD follows it at offset 8.
    bytes.extend_from_slice(&[0x4D, 0x4D, 0x00, 0x2A]);
    bytes.extend_from_slice(&8_u32.to_be_bytes());
    bytes.extend_from_slice(&3_u16.to_be_bytes());
    let mut tag = |id: u16, kind: u16, count: u32, value: [u8; 4]| {
        bytes.extend_from_slice(&id.to_be_bytes());
        bytes.extend_from_slice(&kind.to_be_bytes());
        bytes.extend_from_slice(&count.to_be_bytes());
        bytes.extend_from_slice(&value);
    };
    tag(0xB000, TIFF_UNDEFINED, 4, *b"0100"); // MPFVersion
    tag(0xB001, TIFF_LONG, 1, 2_u32.to_be_bytes()); // NumberOfImages
    tag(0xB002, TIFF_UNDEFINED, 32, MP_ENTRIES_OFFSET.to_be_bytes()); // MPEntry
    // No attribute IFD.
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    for (attribute, size, offset) in [
        (MP_TYPE_BASELINE_PRIMARY, primary_size, 0),
        (MP_TYPE_GAIN_MAP, gain_size, gain_offset),
    ] {
        bytes.extend_from_slice(&attribute.to_be_bytes());
        bytes.extend_from_slice(&size.to_be_bytes());
        bytes.extend_from_slice(&offset.to_be_bytes());
        // No dependent images.
        bytes.extend_from_slice(&[0; 4]);
    }
    bytes
}

/// Where the content of the header-block APP2 segment starting with `label` begins.
/// Walks the `APPn` run after `SOI` and stops at the first other marker, so a segment
/// past the frame header — one no reader would find — is never matched.
fn header_app2(jpeg: &[u8], label: &[u8]) -> Result<usize> {
    let missing = || {
        NcError::Write(format!(
            "the gain-map JPEG's base has no {} segment in its header block",
            String::from_utf8_lossy(label.strip_suffix(b"\0").unwrap_or(label))
        ))
    };
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return Err(missing());
    }
    let mut at = 2;
    while let [0xFF, marker @ 0xE0..=0xEF, high, low, ..] = jpeg[at..] {
        let length = usize::from(u16::from_be_bytes([high, low]));
        let content = at + 4;
        let end = at + 2 + length;
        if length < 2 || end > jpeg.len() {
            return Err(missing());
        }
        if marker == 0xE2 && jpeg[content..end].starts_with(label) {
            return Ok(content);
        }
        at = end;
    }
    Err(missing())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::gain_encode::{self, OFFSET};
    use crate::pipeline::gain_ratio::between;
    use crate::pipeline::hdr::LINEAR_HEADROOM;

    /// The marker sequence of one JPEG image up to its first scan, with a label for
    /// the APP segments that carry one.
    fn header_markers(jpeg: &[u8]) -> Vec<String> {
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let mut out = vec!["SOI".to_string()];
        let mut at = 2;
        loop {
            let marker = jpeg[at + 1];
            let length = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
            let content = &jpeg[at + 4..at + 2 + length];
            out.push(match marker {
                0xE0 if content.starts_with(b"JFIF\0") => "APP0 JFIF".into(),
                0xE2 if content.starts_with(b"ICC_PROFILE\0") => "APP2 ICC".into(),
                0xE2 if content.starts_with(b"urn:iso:std:iso:ts:21496:-1\0") => "APP2 ISO".into(),
                0xE2 if content.starts_with(MPF_LABEL) => "APP2 MPF".into(),
                0xC0 => "SOF0".into(),
                0xDA => "SOS".into(),
                m => format!("{m:02X}"),
            });
            if marker == 0xDA {
                return out;
            }
            at += 2 + length;
        }
    }

    /// A small frame whose red channel carries highlight gain in one quadrant, so the
    /// map is live and chromatic.
    fn sample() -> (Vec<u8>, GainMapImage, metadata::IsoGainMapFields, Vec<f32>) {
        let (width, height) = (16_u32, 16_u32);
        let mut sdr = Vec::new();
        let mut hdr = Vec::new();
        for i in 0..(width * height) as usize {
            let (x, y) = (i as u32 % width, i as u32 / width);
            let base = [0.8, 0.6, 0.4];
            sdr.extend(base);
            if x < 8 && y < 8 {
                hdr.extend([3.0, 0.6, 0.4]);
            } else {
                hdr.extend(base);
            }
        }
        let image = |rgb| LinearImage::new(width, height, rgb, None).unwrap();
        let ratios = between(&image(sdr.clone()), &image(hdr), OFFSET).unwrap();
        let map = gain_encode::encode(&ratios).unwrap();
        let fields = fields(&map, LINEAR_HEADROOM).unwrap();
        let base_rgb: Vec<u8> = sdr.iter().map(|v| (v * 255.0).round() as u8).collect();
        let bytes = assemble(&base_rgb, width, height, &[0; 16], &map, &fields).unwrap();
        (bytes, map, fields, ratios.rgb().to_vec())
    }

    /// `(attribute, size, offset)` of each MP entry, and where offsets are measured from.
    fn mp_entries(bytes: &[u8]) -> (usize, Vec<(u32, u32, u32)>) {
        let label = header_app2(bytes, MPF_LABEL).unwrap();
        let reference = label + MPF_LABEL.len();
        let entries = reference + MP_ENTRIES_OFFSET as usize;
        let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        let parsed = (0..2)
            .map(|n| {
                let at = entries + n * 16;
                (word(at), word(at + 4), word(at + 8))
            })
            .collect();
        (reference, parsed)
    }

    #[test]
    fn every_segment_sits_in_the_header_block_of_its_image() {
        let (bytes, ..) = sample();
        let (reference, entries) = mp_entries(&bytes);
        let base = &bytes[..entries[0].1 as usize];
        let gain = &bytes[reference + entries[1].2 as usize..];
        let base_markers = header_markers(base);
        let prefix = ["SOI", "APP0 JFIF", "APP2 ICC", "APP2 ISO", "APP2 MPF"];
        assert_eq!(base_markers[..5], prefix, "{base_markers:?}");
        assert!(
            base_markers[5..].iter().all(|m| !m.starts_with("APP")),
            "{base_markers:?}"
        );
        assert!(
            base_markers.contains(&"SOF0".to_string()),
            "{base_markers:?}"
        );
        let gain_markers = header_markers(gain);
        assert_eq!(
            gain_markers[..3],
            ["SOI", "APP0 JFIF", "APP2 ISO"],
            "{gain_markers:?}"
        );
        assert!(
            gain_markers[3..].iter().all(|m| !m.starts_with("APP")),
            "{gain_markers:?}"
        );
    }

    #[test]
    fn mpf_entries_resolve_to_both_images_with_their_type_codes() {
        let (bytes, ..) = sample();
        let (reference, entries) = mp_entries(&bytes);
        let [
            (primary_type, primary_size, primary_offset),
            (gain_type, gain_size, gain_offset),
        ] = entries[..]
        else {
            unreachable!()
        };
        assert_eq!((primary_type, primary_offset), (0x03_0000, 0));
        assert_eq!(gain_type, 0x05_0000);
        // The primary ends where the gain map begins, and the gain map ends the file.
        assert_eq!(primary_size as usize, reference + gain_offset as usize);
        assert_eq!(&bytes[primary_size as usize - 2..][..2], &[0xFF, 0xD9]);
        let gain_start = reference + gain_offset as usize;
        assert_eq!(&bytes[gain_start..gain_start + 2], &[0xFF, 0xD8]);
        assert_eq!(gain_start + gain_size as usize, bytes.len());
    }

    #[test]
    fn the_segments_carry_the_iso_payloads_and_no_other_dialect() {
        let (bytes, _, fields, _) = sample();
        let (reference, entries) = mp_entries(&bytes);
        let base = &bytes[..entries[0].1 as usize];
        let gain = &bytes[reference + entries[1].2 as usize..];
        let payload = |jpeg: &[u8]| {
            let content = header_app2(jpeg, b"urn:iso:std:iso:ts:21496:-1\0").unwrap();
            let length = usize::from(u16::from_be_bytes([jpeg[content - 2], jpeg[content - 1]]));
            jpeg[content + 28..content - 2 + length].to_vec()
        };
        assert_eq!(payload(base), metadata::serialize_version(&fields));
        assert_eq!(
            payload(gain),
            metadata::serialize_metadata(&fields).unwrap()
        );
        // No legacy XMP, no Exif: the only dialect is ISO's.
        let contains = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
        assert!(!contains(b"hdrgm"));
        assert!(!contains(b"http://ns.adobe.com/xap/1.0/"));
        assert!(!contains(b"Exif\0\0"));
    }

    #[test]
    fn the_metadata_states_three_channels_and_the_declared_headroom() {
        let (_, map, fields, _) = sample();
        assert!(fields.is_multichannel && fields.use_base_colour_space);
        assert_eq!(fields.base_hdr_headroom_log2.value(), 0.0);
        let headroom = f64::from(LINEAR_HEADROOM).log2();
        assert!((fields.alternate_hdr_headroom_log2.value() - headroom).abs() < 1e-6);
        // Red is live, green and blue are flat: one window each.
        assert!(fields.gain_map_max_log2[0].value() > 1.0);
        for c in 1..3 {
            assert_eq!(fields.gain_map_min_log2[c].value(), 0.0);
            assert_eq!(fields.gain_map_max_log2[c].value(), 0.0);
        }
        for c in 0..3 {
            assert!(
                (fields.base_offset[c].value() - f64::from(OFFSET)).abs() < 1e-9
                    && (fields.alternate_offset[c].value() - f64::from(OFFSET)).abs() < 1e-9
            );
            assert!(
                (fields.gain_map_max_log2[c].value() - f64::from(map.window.log2_max[c])).abs()
                    < 1e-6
            );
        }
    }

    #[test]
    fn both_images_decode_and_the_gain_map_rebuilds_the_gains() {
        let (bytes, map, _, ratios) = sample();
        let (reference, entries) = mp_entries(&bytes);
        // An ordinary reader opens the file as the SDR base alone.
        let base = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(base.dimensions(), (16, 16));
        let gain = image::load_from_memory(&bytes[reference + entries[1].2 as usize..])
            .unwrap()
            .to_rgb8();
        assert_eq!(gain.dimensions(), (map.width, map.height));
        // Each 2×2 source block is one constant gain, so every decoded sample lands
        // near the block's log2 gain. The bound is the codec's, on the worst case: the
        // whole 8×8 map is one DCT block holding a hard 0 → 255 step in red, which
        // q95 and the RGB → YCbCr conversion ring by ~5 codes.
        for (x, y, px) in gain.enumerate_pixels() {
            let source = ((2 * y * 16 + 2 * x) * 3) as usize;
            for c in 0..3 {
                let want = ratios[source + c].log2();
                let got = map.window.decode(c, px[c]);
                let span = map.window.log2_max[c] - map.window.log2_min[c];
                assert!(
                    (got - want).abs() <= span * 8.0 / 255.0 + 1e-6,
                    "({x}, {y}) channel {c}: {got} vs {want}"
                );
            }
        }
    }

    #[test]
    fn a_base_without_mpf_in_its_header_is_refused() {
        let jpeg = encode_jpeg(&[0; 3], 1, 1, None, "t", ColorType::Rgb, Vec::new()).unwrap();
        let err = header_app2(&jpeg, MPF_LABEL).unwrap_err();
        assert!(err.to_string().contains("no MPF segment"), "{err}");
        assert_eq!(mpf_content(1, 2, 3).len(), mpf_content(0, 0, 0).len());
    }
}
