//! Input/output stages: decode scanner files into [`LinearImage`] and encode
//! results out to TIFF. These are the only places crate-specific image/TIFF
//! types appear; everything else speaks the neutral types in [`crate::types`].

/// Samples per rayon band in this module's parallel sample loops — the two
/// quantizers and `encode::channel_means_u16`. A **multiple of 3**, so a sample's
/// channel is its index within the band modulo 3: every band therefore starts on a
/// red sample, which is what lets the per-channel sums be folded per band. It lives
/// here rather than in either encoder because both use it, and it is unrelated to
/// `pipeline::color`'s row band, which is sized by lcms2's per-call setup cost.
pub(crate) const QUANTIZE_BAND_SAMPLES: usize = 3 * (1 << 15);
const _: () = assert!(
    QUANTIZE_BAND_SAMPLES.is_multiple_of(3),
    "a band that is not a multiple of 3 misaligns every per-channel sum folded per band"
);

pub mod avif;
pub mod decode;
pub mod encode;
/// Temp-write → fsync → rename, so no truncated file ever appears at a final path.
/// Every writer in this module goes through it; see the module docs for the exact
/// guarantee (per-file atomicity, not a multi-file transaction).
pub mod staged;
pub mod ultra_hdr;
