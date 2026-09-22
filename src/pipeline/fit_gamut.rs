//! **Stage 4 of the new rendering chain — fit gamut.** Identity for now.
//!
//! Move out-of-gamut colour to the display's boundary, keeping hue.
//! `nf-display-stages/fit-gamut` fills it, with one implementation replacing the
//! three that exist today.
//!
//! This is the chain's last stage: its output is what the encoder quantizes, and
//! the encode boundary is the **only** place clamping happens — every stage above
//! passes the working range through unclamped.

use std::fmt;

use crate::pipeline::fit_range::RangeFittedImage;
use crate::pipeline::working_image::WorkingBuffer;
use crate::types::{LinearImage, Result};

/// Fit gamut's knobs. Empty until `nf-display-stages/fit-gamut` gives it a
/// boundary and a mapping. See [`SceneCorrectionParams`] on why this is an empty
/// struct rather than an `Option`.
///
/// [`SceneCorrectionParams`]: crate::pipeline::scene_correction::SceneCorrectionParams
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FitGamutParams {}

/// The chain's output: **display-referred** pixels, in gamut, ready to encode.
///
/// [`into_linear`] is the whole boundary: the chain's **exit**, and it moves, so
/// leaving the chain costs no more than entering it did. Nothing else is exposed —
/// a destination consumes the `LinearImage` and reads its public fields, so a
/// parallel set of borrowing accessors would only be a second way to say the same
/// thing (and the borrow and the move cannot both be used by one consumer anyway).
///
/// **The carried IR plane rides out with it.** The design says carry the plane
/// through rather than consume it (CLAUDE.md), and this chain must not be the thing
/// that loses it — today's `pipeline::sdr` render does drop it
/// (`LinearImage::new(w, h, rgb, None)`), and the new chain deliberately does not
/// copy that. Nothing downstream depends on the plane arriving here: `--export-ir`
/// writes from the *decoded* image, pre-render, and both IR warnings are derived
/// before the render too. How the plane *travels* is `nf-core/buffer-strategy`'s to
/// settle; that it is not lost is decided here.
///
/// [`into_linear`]: Self::into_linear
pub struct DisplayReferredImage(WorkingBuffer);

impl DisplayReferredImage {
    /// Leave the chain, handing the buffers to the encoder.
    ///
    /// `io::encode` takes `&LinearImage`, so this is how a destination gets one
    /// **without copying** a full-frame buffer. Consuming, mirroring
    /// `AcesCgImage::into_linear` at the other end of the chain.
    #[allow(dead_code)] // the encode hand-off, wired by `nf-core/minimal-end-to-end`.
    pub(crate) fn into_linear(self) -> LinearImage {
        self.0.into_linear()
    }
}

impl fmt::Debug for DisplayReferredImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_named(f, "DisplayReferredImage")
    }
}

/// Fit the colour to the display's gamut — **identity today**, and bit-exactly so.
///
/// Fallible by construction, not by need: see [`chain::render`].
///
/// [`chain::render`]: crate::pipeline::chain::render
pub fn apply(image: RangeFittedImage, _params: &FitGamutParams) -> Result<DisplayReferredImage> {
    Ok(DisplayReferredImage(image.into_buffer()))
}
