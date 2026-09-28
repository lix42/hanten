//! ISO 21496-1 gain-map metadata: the field set, and the bytes it serializes to.
//!
//! This module owns the ISO dialect's numeric field set, built from what a map
//! states ([`fields`], from a [`Stated`]); the Annex C.2.2 byte serializers built on
//! that field set ([`serialize_version`], [`serialize_metadata`]); and the C.4.6 APP2
//! payload framing ([`segment_content`]). It does **not** place the segments in a
//! file: placement is the container's, [`super`]'s.
//!
//! **Field layout and semantics are pinned against the licensed
//! ISO 21496-1:2025 text** (Annex C.2, normative), not inferred from an
//! implementation. Every structural claim below traces to a numbered subclause,
//! cited inline so a future reader can re-check it against their own copy. No
//! normative text is reproduced here — only our own restatement of the layout,
//! which is what the licence permits.
//!
//! **Two deliberate divergences from the reference implementation**, both found
//! by reading the standard and both load-bearing:
//!
//! 1. **No common-denominator compact form.** `libultrahdr` sets a flag bit and
//!    emits a shortened layout whenever every denominator matches. The normative
//!    structure has no such flag — bits 5..0 of the flags byte are `reserved` —
//!    and always spells out each numerator/denominator pair. nc's uniform
//!    offsets and `gamma = 1` would trigger that compact path, so reusing the
//!    reference serializer would emit a non-conformant payload in the common
//!    case. Never add the compact form back.
//! 2. **No `backward_direction` field.** The reference implementation writes one;
//!    the structure has no place for it. Direction is instead carried by
//!    `sign(H_alternate − H_baseline)` in the application formula (Annex A.2,
//!    Clause 6.3). For nc that sign is always positive: the SDR base sits at
//!    `H_baseline = 0` and the HDR alternate above it.
//!
//! The identifier is **not** draft-era despite appearances: C.3 and the C.4.6
//! segment table both specify the label `urn:iso:std:iso:ts:21496:-1` for the
//! published first edition. Reading the `ts:` as evidence of a stale
//! implementation was an error this module previously recorded.
//!
//! **Determinism scope.** Field values pass through `log2`, and a 1-ulp
//! transcendental difference can move a continued-fraction expansion to a very
//! different numerator/denominator pair. ISO metadata bytes are therefore
//! promised byte-identical only per pinned build/architecture, matching the
//! spike note's "Determinism and acceptance" scope — never as a cross-platform
//! contract.

use crate::types::{NcError, Result};

/// The SDR base sits at reference white by construction, so its headroom is
/// `1.0` linear and `0` in the log2 domain the ISO fields use. Product policy
/// from the spike's rendering contract, not colorimetry.
const BASE_HDR_HEADROOM_LINEAR: f32 = 1.0;

/// The worst case for a continued-fraction expansion is the golden ratio, which
/// converges in 39 terms; the reference implementation uses the same bound.
const MAX_TERMS: usize = 39;

/// A signed rational, as the ISO metadata's signed numeric fields store one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rational {
    pub numerator: i32,
    pub denominator: u32,
}

/// An unsigned rational, as the ISO metadata's unsigned numeric fields store one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UnsignedRational {
    pub numerator: u32,
    pub denominator: u32,
}

impl Rational {
    /// Represent `value` exactly where possible, else as the closest rational
    /// within the signed field's range.
    pub(crate) fn from_f32(value: f32) -> Result<Self> {
        let (numerator, denominator) = approximate(value.abs(), i32::MAX as u64)?;
        let numerator = i32::try_from(numerator).map_err(|_| {
            NcError::Other(format!(
                "ISO gain-map field {value} does not fit a signed 32-bit numerator"
            ))
        })?;
        Ok(Self {
            numerator: if value.is_sign_negative() {
                -numerator
            } else {
                numerator
            },
            denominator,
        })
    }

    /// The represented value. The serializer writes the pair, not this — but the
    /// standard's constraints (5.2.5.3, 5.2.7) are stated on values, so
    /// validation and dual-dialect agreement checks both need it.
    pub(crate) fn value(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }
}

impl UnsignedRational {
    /// Represent a non-negative `value` exactly where possible, else as the
    /// closest rational within the unsigned field's range.
    pub(crate) fn from_f32(value: f32) -> Result<Self> {
        if value.is_sign_negative() && value != 0.0 {
            return Err(NcError::Other(format!(
                "ISO gain-map unsigned field cannot represent the negative value {value}"
            )));
        }
        let (numerator, denominator) = approximate(value, u64::from(u32::MAX))?;
        let numerator = u32::try_from(numerator).map_err(|_| {
            NcError::Other(format!(
                "ISO gain-map field {value} does not fit an unsigned 32-bit numerator"
            ))
        })?;
        Ok(Self {
            numerator,
            denominator,
        })
    }

    /// The represented value, for the same reason as [`Rational::value`].
    pub(crate) fn value(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }
}

/// The `GainMapMetadata` field set of Annex C.2.2, projected from the canonical
/// model. Field names follow the standard's own identifiers.
///
/// nc always writes three metadata channels, so `channel_count` is fixed at 3
/// and `is_multichannel` at `true`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IsoGainMapFields {
    /// The minimum parser version. C.2.3 and 5.2.8: zero for this edition.
    pub minimum_version: u16,
    /// The writing implementation's version; C.2.3 requires
    /// `>= minimum_version`.
    pub writer_version: u16,
    /// Whether the *per-channel metadata* count is 3 rather than 1. C.2.3 notes
    /// this may differ from the gain map's own channel count — it describes the
    /// metadata, not the image.
    pub is_multichannel: bool,
    /// Whether the gain-map application space takes the baseline image's
    /// primaries (5.3.4, Annex B.2).
    pub use_base_colour_space: bool,
    /// `log2` of the per-channel minimum gain, i.e. `min(G)` (5.2.5.2).
    pub gain_map_min_log2: [Rational; 3],
    /// `log2` of the per-channel maximum gain, i.e. `max(G)` (5.2.5.3), which
    /// 5.2.5.3 requires to be `>= min(G)`.
    pub gain_map_max_log2: [Rational; 3],
    /// Per-channel encoding gamma (5.2.5.6), which must be strictly positive.
    pub gain_map_gamma: [UnsignedRational; 3],
    /// Per-channel baseline offset `k_baseline` (5.2.5.4).
    pub base_offset: [Rational; 3],
    /// Per-channel alternate offset `k_alternate` (5.2.5.5).
    pub alternate_offset: [Rational; 3],
    /// `H_baseline` (5.2.6) — zero for an SDR base, which sits at reference
    /// white by construction.
    pub base_hdr_headroom_log2: UnsignedRational,
    /// `H_alternate` (5.2.7), which 5.2.7 requires to differ from
    /// `H_baseline`.
    pub alternate_hdr_headroom_log2: UnsignedRational,
}

/// The values an ISO gain map states, in the units its fields store them: the gain
/// extrema as `log2`, gamma and the offsets linear. The headroom is the one linear
/// value converted here, because every caller holds it as a display ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Stated {
    pub gain_min_log2: [f32; 3],
    pub gain_max_log2: [f32; 3],
    pub gamma: [f32; 3],
    pub base_offset: [f32; 3],
    pub alternate_offset: [f32; 3],
    /// The alternate rendition's peak over the base's reference white, linear.
    pub alternate_headroom_linear: f32,
}

/// The ISO field set for what a gain map states, validated.
///
/// Always three metadata channels over the baseline's primaries: the map is derived
/// in the base rendition's own gamut, and C.2.3 lets the metadata channel count differ
/// from the map's.
pub(crate) fn fields(stated: &Stated) -> Result<IsoGainMapFields> {
    let mut gain_map_min_log2 = [Rational {
        numerator: 0,
        denominator: 1,
    }; 3];
    let mut gain_map_max_log2 = gain_map_min_log2;
    let mut base_offset = gain_map_min_log2;
    let mut alternate_offset = gain_map_min_log2;
    let mut gain_map_gamma = [UnsignedRational {
        numerator: 1,
        denominator: 1,
    }; 3];

    for channel in 0..3 {
        gain_map_min_log2[channel] = Rational::from_f32(stated.gain_min_log2[channel])?;
        gain_map_max_log2[channel] = Rational::from_f32(stated.gain_max_log2[channel])?;
        gain_map_gamma[channel] = UnsignedRational::from_f32(stated.gamma[channel])?;
        base_offset[channel] = Rational::from_f32(stated.base_offset[channel])?;
        alternate_offset[channel] = Rational::from_f32(stated.alternate_offset[channel])?;
    }

    let fields = IsoGainMapFields {
        // 5.2.8: zero for this edition; C.2.3: writer >= minimum.
        minimum_version: 0,
        writer_version: 0,
        is_multichannel: true,
        use_base_colour_space: true,
        gain_map_min_log2,
        gain_map_max_log2,
        gain_map_gamma,
        base_offset,
        alternate_offset,
        base_hdr_headroom_log2: UnsignedRational::from_f32(log2_positive(
            BASE_HDR_HEADROOM_LINEAR,
            "base headroom",
        )?)?,
        alternate_hdr_headroom_log2: UnsignedRational::from_f32(log2_positive(
            stated.alternate_headroom_linear,
            "alternate headroom",
        )?)?,
    };
    validate_fields(&fields)?;
    Ok(fields)
}

/// Enforce the constraints the standard states on the field values themselves,
/// so a malformed payload fails here rather than in someone's decoder.
fn validate_fields(fields: &IsoGainMapFields) -> Result<()> {
    // C.2.3: writer_version >= minimum_version.
    if fields.writer_version < fields.minimum_version {
        return Err(NcError::Other(format!(
            "ISO gain-map writer_version {} is below minimum_version {}",
            fields.writer_version, fields.minimum_version
        )));
    }
    // 5.2.7: the two headrooms must differ, else the weighting factor of
    // Clause 6.3 divides by zero. Compared as values, not as pairs: `0/1` and
    // `0/2` are distinct pairs denoting the same headroom.
    if fields.base_hdr_headroom_log2.value() == fields.alternate_hdr_headroom_log2.value() {
        return Err(NcError::Other(
            "ISO gain-map baseline and alternate HDR headroom must differ".to_string(),
        ));
    }
    // C.2.3: no denominator may be zero.
    let denominators = fields
        .gain_map_min_log2
        .iter()
        .chain(&fields.gain_map_max_log2)
        .chain(&fields.base_offset)
        .chain(&fields.alternate_offset)
        .map(|rational| rational.denominator)
        .chain(
            fields
                .gain_map_gamma
                .iter()
                .map(|rational| rational.denominator),
        )
        .chain([
            fields.base_hdr_headroom_log2.denominator,
            fields.alternate_hdr_headroom_log2.denominator,
        ]);
    for denominator in denominators {
        if denominator == 0 {
            return Err(NcError::Other(
                "ISO gain-map field denominators must be non-zero".to_string(),
            ));
        }
    }
    for channel in 0..3 {
        // 5.2.5.3: max(G) >= min(G), compared as values because the pairs may
        // carry different denominators.
        if fields.gain_map_max_log2[channel].value() < fields.gain_map_min_log2[channel].value() {
            return Err(NcError::Other(format!(
                "ISO gain-map max(G) is below min(G) on channel {channel}"
            )));
        }
        // 5.2.5.6 and C.2.3: gamma is strictly positive, and its numerator is
        // separately required to be non-zero.
        if fields.gain_map_gamma[channel].numerator == 0 {
            return Err(NcError::Other(format!(
                "ISO gain-map gamma numerator must be non-zero on channel {channel}"
            )));
        }
    }
    Ok(())
}

/// The segment label of C.3 and the C.4.6 layout table. The published first
/// edition specifies the `ts:` form — this is **not** a draft-era identifier.
/// Stored with its null terminator, which the table's 28-byte length includes.
const SEGMENT_LABEL: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";

/// The label plus payload, i.e. an APP2 segment's *content* after its length
/// field — the form a JPEG encoder that frames segments itself wants.
pub(crate) fn segment_content(payload: &[u8]) -> Vec<u8> {
    let mut content = Vec::with_capacity(SEGMENT_LABEL.len() + payload.len());
    content.extend_from_slice(SEGMENT_LABEL);
    content.extend_from_slice(payload);
    content
}

/// Serialize the `GainMapVersion` structure of C.2.2 — the payload C.4.3
/// requires in the **baseline** image's segment, where the full metadata
/// structure must not appear.
pub(crate) fn serialize_version(fields: &IsoGainMapFields) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(4);
    bytes.extend_from_slice(&fields.minimum_version.to_be_bytes());
    bytes.extend_from_slice(&fields.writer_version.to_be_bytes());
    bytes
}

/// Serialize the full `GainMapMetadata` structure of C.2.2 — the payload the
/// **gain-map** image's segment carries (C.4.6).
///
/// Big-endian throughout, independent of the container (C.2.1). Deliberately
/// *without* the reference implementation's common-denominator compact form,
/// which the structure has no flag for; see the module note.
pub(crate) fn serialize_metadata(fields: &IsoGainMapFields) -> Result<Vec<u8>> {
    validate_fields(fields)?;

    let mut bytes = serialize_version(fields);

    // C.2.1: sub-byte fields take bits from most to least significant, so
    // `is_multichannel` is bit 7 and `use_base_colour_space` bit 6. Bits 5..0
    // are reserved and must be written as zero.
    let mut flags = 0_u8;
    if fields.is_multichannel {
        flags |= 1 << 7;
    }
    if fields.use_base_colour_space {
        flags |= 1 << 6;
    }
    bytes.push(flags);

    for headroom in [
        fields.base_hdr_headroom_log2,
        fields.alternate_hdr_headroom_log2,
    ] {
        bytes.extend_from_slice(&headroom.numerator.to_be_bytes());
        bytes.extend_from_slice(&headroom.denominator.to_be_bytes());
    }

    // C.2.3: when multichannel, the channel order is R, G, B.
    let channel_count = if fields.is_multichannel { 3 } else { 1 };
    for channel in 0..channel_count {
        for signed in [
            fields.gain_map_min_log2[channel],
            fields.gain_map_max_log2[channel],
        ] {
            bytes.extend_from_slice(&signed.numerator.to_be_bytes());
            bytes.extend_from_slice(&signed.denominator.to_be_bytes());
        }
        bytes.extend_from_slice(&fields.gain_map_gamma[channel].numerator.to_be_bytes());
        bytes.extend_from_slice(&fields.gain_map_gamma[channel].denominator.to_be_bytes());
        for signed in [
            fields.base_offset[channel],
            fields.alternate_offset[channel],
        ] {
            bytes.extend_from_slice(&signed.numerator.to_be_bytes());
            bytes.extend_from_slice(&signed.denominator.to_be_bytes());
        }
    }

    Ok(bytes)
}

/// `log2` of a strictly positive linear value, in `f64` then narrowed — the
/// reference implementation's `log2(float)` promotes the same way.
fn log2_positive(value: f32, name: &str) -> Result<f32> {
    if !value.is_finite() || value <= 0.0 {
        return Err(NcError::Other(format!(
            "ISO gain-map {name} must be finite and positive to take log2 (got {value})"
        )));
    }
    Ok(f64::from(value).log2() as f32)
}

/// Best rational approximation of a non-negative value by continued fractions.
///
/// Follows the pinned reference implementation's convention so nc's fields land
/// where existing decoders expect them, with two deliberate differences: `f64`
/// internals throughout, and an explicit error rather than a silent best-effort
/// result when the value cannot be represented at all.
fn approximate(value: f32, max_numerator: u64) -> Result<(u64, u32)> {
    let limit = max_numerator as f64;
    let value = f64::from(value);
    if !value.is_finite() || value < 0.0 || value > limit {
        return Err(NcError::Other(format!(
            "ISO gain-map field value {value} is not a representable non-negative rational"
        )));
    }

    // The largest denominator that still keeps the numerator inside the field.
    let max_denominator = if value <= 1.0 {
        f64::from(u32::MAX)
    } else {
        (limit / value).floor()
    };

    let mut denominator = 1.0_f64;
    let mut previous_denominator = 0.0_f64;
    let mut remainder = value - value.floor();
    for _ in 0..MAX_TERMS {
        let numerator = denominator * value;
        if numerator > limit {
            return Err(NcError::Other(format!(
                "ISO gain-map field value {value} overflows the field's numerator"
            )));
        }
        let rounded = numerator.round();
        if numerator == rounded {
            return Ok((rounded as u64, denominator as u32));
        }
        if remainder == 0.0 {
            // Unreachable for an integral value, which terminates exactly above.
            return Ok((rounded as u64, denominator as u32));
        }
        remainder = 1.0 / remainder;
        let next = previous_denominator + remainder.floor() * denominator;
        if next > max_denominator || next > f64::from(u32::MAX) {
            // The closest we can get inside the field's denominator range.
            return Ok((rounded as u64, denominator as u32));
        }
        previous_denominator = denominator;
        denominator = next;
        remainder -= remainder.floor();
    }

    Ok(((denominator * value).round() as u64, denominator as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::hdr::LINEAR_HEADROOM;

    /// What a per-channel map over the default HDR destination states: distinct
    /// extrema per channel, the offsets the map is ratioed with, unit gamma.
    fn stated() -> Stated {
        Stated {
            gain_min_log2: [-0.5, 0.0, 0.25],
            gain_max_log2: [1.5, 2.0, 1.75],
            gamma: [1.0; 3],
            base_offset: [1.0 / 64.0; 3],
            alternate_offset: [1.0 / 64.0; 3],
            alternate_headroom_linear: LINEAR_HEADROOM,
        }
    }

    fn sample() -> IsoGainMapFields {
        fields(&stated()).unwrap()
    }

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 2e-6, "{actual} != {expected}");
    }

    #[test]
    fn dyadic_values_are_represented_exactly_with_small_denominators() {
        // Every value nc actually emits as an offset or gamma is dyadic, so the
        // expansion must terminate exactly rather than approximate.
        for (value, numerator, denominator) in [
            (0.0_f32, 0, 1),
            (1.0, 1, 1),
            (2.0, 2, 1),
            (0.5, 1, 2),
            (0.75, 3, 4),
            (1.0 / 64.0, 1, 64),
            (1.0 / 256.0, 1, 256),
        ] {
            let signed = Rational::from_f32(value).unwrap();
            assert_eq!(
                (signed.numerator, signed.denominator),
                (numerator, denominator),
                "signed {value}"
            );
            let unsigned = UnsignedRational::from_f32(value).unwrap();
            assert_eq!(
                (unsigned.numerator, unsigned.denominator),
                (numerator as u32, denominator),
                "unsigned {value}"
            );
        }
    }

    #[test]
    fn signed_fields_keep_the_sign_on_the_numerator() {
        let negative = Rational::from_f32(-1.0 / 64.0).unwrap();
        assert_eq!((negative.numerator, negative.denominator), (-1, 64));
        close(negative.value(), -1.0 / 64.0);
    }

    #[test]
    fn unsigned_fields_reject_negative_values() {
        let error = UnsignedRational::from_f32(-0.5).unwrap_err();
        assert!(error.to_string().contains("cannot represent the negative"));
        // Negative zero is still zero, and must not be rejected.
        assert_eq!(UnsignedRational::from_f32(-0.0).unwrap().numerator, 0);
    }

    #[test]
    fn non_finite_values_fail_instead_of_producing_a_field() {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(Rational::from_f32(invalid).is_err(), "{invalid}");
            assert!(UnsignedRational::from_f32(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn inexact_values_round_trip_within_the_field_resolution() {
        // 0.1 and log2(1000/203) are not dyadic; the expansion must still land
        // close enough that a decoder recovers the intended value.
        for value in [0.1_f32, 2.300_448, 1.0 / 3.0, LINEAR_HEADROOM] {
            let signed = Rational::from_f32(value).unwrap();
            close(signed.value(), f64::from(value));
            assert!(signed.denominator > 0);
        }
    }

    #[test]
    fn the_fields_store_what_the_map_states() {
        // The extrema are already log2 and pass through; gamma and the offsets are
        // linear and pass through; only the headroom is converted, from a ratio.
        let fields = sample();
        let stated = stated();
        for channel in 0..3 {
            close(
                fields.gain_map_min_log2[channel].value(),
                f64::from(stated.gain_min_log2[channel]),
            );
            close(
                fields.gain_map_max_log2[channel].value(),
                f64::from(stated.gain_max_log2[channel]),
            );
            close(fields.gain_map_gamma[channel].value(), 1.0);
            close(fields.base_offset[channel].value(), 1.0 / 64.0);
            close(fields.alternate_offset[channel].value(), 1.0 / 64.0);
        }
        assert!(fields.is_multichannel && fields.use_base_colour_space);
    }

    #[test]
    fn headroom_fields_pin_the_sdr_base_at_zero_and_the_peak_at_the_spike_capacity() {
        let fields = sample();

        // An SDR base sits at reference white: 1.0 linear, 0 in log2.
        assert_eq!(fields.base_hdr_headroom_log2.numerator, 0);
        // The alternate headroom is the spike's pinned 2.300448 log2 capacity,
        // not its 4.926108 linear form.
        close(fields.alternate_hdr_headroom_log2.value(), 2.300_448_4);
        assert!((fields.alternate_hdr_headroom_log2.value() - 4.926_108).abs() > 2.0);
    }

    #[test]
    fn non_positive_gains_fail_before_a_log2_field_is_built() {
        let error = log2_positive(0.0, "gain minimum").unwrap_err();
        assert!(error.to_string().contains("finite and positive"));
        assert!(log2_positive(-1.0, "gain maximum").is_err());
        assert!(log2_positive(f32::NAN, "gain maximum").is_err());
    }

    #[test]
    fn segment_label_matches_the_published_length_and_identifier() {
        // C.4.6's table gives 28 bytes: a 27-byte label plus one null. That
        // arithmetic is the check that we transcribed the identifier exactly —
        // a typo in the URN would change the length.
        assert_eq!(SEGMENT_LABEL.len(), 28);
        assert_eq!(SEGMENT_LABEL[27], 0);
        assert_eq!(&SEGMENT_LABEL[..27], b"urn:iso:std:iso:ts:21496:-1");
    }

    #[test]
    fn metadata_payload_has_the_size_the_normative_structure_implies() {
        let fields = sample();
        let payload = serialize_metadata(&fields).unwrap();

        // 4 version + 1 flags + 4 headroom rationals (16) + 3 channels of 5
        // rationals (3 x 40) = 141. Derived from C.2.2's field list, so it fails
        // if a field is dropped, doubled, or silently resized.
        assert_eq!(payload.len(), 4 + 1 + 16 + 3 * 40);
        // The baseline image carries only GainMapVersion (C.4.3).
        assert_eq!(serialize_version(&fields).len(), 4);
    }

    #[test]
    fn payload_is_big_endian_with_reserved_flag_bits_clear() {
        let fields = sample();
        let payload = serialize_metadata(&fields).unwrap();

        // minimum_version and writer_version are both zero for this edition.
        assert_eq!(&payload[..4], &[0, 0, 0, 0]);
        // is_multichannel is bit 7, use_base_colour_space bit 6, and C.2.2's
        // remaining six bits are reserved — they must be zero.
        assert_eq!(payload[4], 0b1100_0000);

        // H_baseline is zero, written as a big-endian 0/1 rational.
        assert_eq!(&payload[5..13], &[0, 0, 0, 0, 0, 0, 0, 1]);
        // H_alternate's numerator is big-endian: its most significant byte
        // leads, so a little-endian slip would put a zero first here.
        let numerator = u32::from_be_bytes([payload[13], payload[14], payload[15], payload[16]]);
        assert_eq!(numerator, fields.alternate_hdr_headroom_log2.numerator);
        assert_ne!(payload[13..17], [0, 0, 0, 0]);
    }

    #[test]
    fn payload_never_uses_the_reference_implementations_compact_form() {
        // With uniform offsets and gamma every denominator matches, which is
        // exactly when `libultrahdr` switches to its shortened layout. The
        // normative structure has no flag for that, so the length must not move.
        let fields = sample();
        assert_eq!(
            fields.base_offset[0].denominator,
            fields.alternate_offset[0].denominator
        );
        assert_eq!(serialize_metadata(&fields).unwrap().len(), 4 + 1 + 16 + 120);
    }

    #[test]
    fn equal_headrooms_are_rejected_as_the_standard_requires() {
        let mut fields = sample();
        fields.alternate_hdr_headroom_log2 = fields.base_hdr_headroom_log2;
        let error = serialize_metadata(&fields).unwrap_err();
        assert!(error.to_string().contains("headroom must differ"));
    }

    #[test]
    fn malformed_fields_are_rejected_before_serialization() {
        let valid = sample();

        // 5.2.5.3: max(G) >= min(G).
        let mut swapped = valid;
        swapped.gain_map_max_log2[1] = Rational {
            numerator: -99,
            denominator: 1,
        };
        assert!(
            serialize_metadata(&swapped)
                .unwrap_err()
                .to_string()
                .contains("max(G) is below min(G)")
        );

        // C.2.3: gamma_numerator shall not be zero.
        let mut zero_gamma = valid;
        zero_gamma.gain_map_gamma[2].numerator = 0;
        assert!(
            serialize_metadata(&zero_gamma)
                .unwrap_err()
                .to_string()
                .contains("gamma numerator must be non-zero")
        );

        // C.2.3: no denominator may be zero.
        let mut zero_denominator = valid;
        zero_denominator.base_offset[0].denominator = 0;
        assert!(
            serialize_metadata(&zero_denominator)
                .unwrap_err()
                .to_string()
                .contains("denominators must be non-zero")
        );

        // C.2.3: writer_version >= minimum_version.
        let mut stale_writer = valid;
        stale_writer.minimum_version = 1;
        assert!(
            serialize_metadata(&stale_writer)
                .unwrap_err()
                .to_string()
                .contains("below minimum_version")
        );
    }
}
