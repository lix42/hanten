//! **Stage 4 of the new rendering chain — fit gamut.**
//!
//! Move out-of-gamut colour to the destination's boundary, keeping hue.
//! `nf-display-stages/fit-gamut` fills it, with one implementation replacing the
//! three that exist today.
//!
//! **What it does today is the half that cannot wait: the change of primaries.**
//! Mapping colour to a destination's boundary presupposes being in that
//! destination's primaries, so the stage owns the matrix out of ACEScg too — and a
//! destination could not otherwise declare a profile that matches its pixels. The
//! mapping itself is not here yet: after the matrix, a colour outside the target
//! gamut is simply outside it (a negative channel, or one above `1.0`), and the
//! encoder clamps and counts it. `nf-display-stages/fit-gamut` adds the radial map
//! on top of this matrix; nothing about the boundary has to move for it.
//!
//! This is the chain's last stage: its output is what the encoder quantizes, and
//! the encode boundary is the **only** place clamping happens — every stage above
//! passes the working range through unclamped.

use std::fmt;

use crate::pipeline::colorimetry::pinned::ACESCG_TO_DISPLAY_P3;
use crate::pipeline::fit_range::RangeFittedImage;
use crate::pipeline::pixels;
use crate::pipeline::working_image::WorkingBuffer;
use crate::types::{LinearImage, Result};

/// The gamut a destination renders into — which primaries the chain's output is in.
///
/// One variant, because there is one destination (`nf-core/minimal-end-to-end`);
/// `nf-destinations/preset-set` adds the rest, Adobe RGB among them. An enum rather
/// than a matrix field so the value can travel with the image to the encoder, which
/// reads it off [`DisplayReferredImage`] instead of re-deriving it from a preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestinationGamut {
    /// Display P3 primaries, D65 white.
    DisplayP3,
}

impl DestinationGamut {
    /// Linear ACEScg → linear destination primaries, from `colorimetry::pinned`.
    fn acescg_matrix(self) -> [[f32; 3]; 3] {
        match self {
            DestinationGamut::DisplayP3 => ACESCG_TO_DISPLAY_P3,
        }
    }

    /// The identifier the report states.
    pub fn name(self) -> &'static str {
        match self {
            DestinationGamut::DisplayP3 => "display-p3",
        }
    }
}

/// Fit gamut's knobs. **No `Default`**: the target gamut is the destination's to
/// state, and a default would let a chain render into primaries nobody chose.
#[derive(Clone, Debug, PartialEq)]
pub struct FitGamutParams {
    pub target: DestinationGamut,
}

impl FitGamutParams {
    /// What fit gamut does under these parameters, for the report: today the change
    /// of primaries alone, with no mapping to the boundary (see the module docs).
    pub fn applied(&self) -> &'static str {
        match self.target {
            DestinationGamut::DisplayP3 => "acescg-to-display-p3-matrix",
        }
    }
}

/// The chain's output: **display-referred** pixels in the destination's primaries,
/// ready to encode.
///
/// [`into_parts`] is the whole boundary: the chain's **exit**, and it moves, so
/// leaving the chain costs no more than entering it did. The gamut rides out beside
/// the pixels, so the encoder learns which primaries it holds from the image itself.
///
/// **The carried IR plane rides out with it.** The design says carry the plane
/// through rather than consume it (CLAUDE.md), and this chain must not be the thing
/// that loses it — today's `pipeline::sdr` render does drop it
/// (`LinearImage::new(w, h, rgb, None)`), and the new chain deliberately does not
/// copy that. Nothing downstream depends on the plane arriving here: `--export-ir`
/// writes from the *decoded* image, and both IR warnings are derived before the
/// render. How the plane *travels* is `nf-core/buffer-strategy`'s to settle; that it
/// is not lost is decided here.
///
/// [`into_parts`]: Self::into_parts
pub struct DisplayReferredImage(WorkingBuffer, DestinationGamut);

impl DisplayReferredImage {
    /// Leave the chain, handing the buffers and their gamut to the encoder.
    ///
    /// `io::encode` takes `&LinearImage`, so this is how a destination gets one
    /// **without copying** a full-frame buffer. Consuming, mirroring
    /// `AcesCgImage::into_linear` at the other end of the chain.
    pub(crate) fn into_parts(self) -> (LinearImage, DestinationGamut) {
        (self.0.into_linear(), self.1)
    }
}

impl fmt::Debug for DisplayReferredImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_named(f, "DisplayReferredImage")
    }
}

/// Move the pixels into the target gamut's primaries.
///
/// A pure 3×3 per pixel, in place: unclamped, so a colour outside the target stays
/// outside it for the encoder to count. Its input is finite — fit range refuses a
/// non-finite sample — so nothing here has to decide what one would mean. Fallible
/// by construction, not by need: see [`chain::render`].
///
/// [`chain::render`]: crate::pipeline::chain::render
pub fn apply(image: RangeFittedImage, params: &FitGamutParams) -> Result<DisplayReferredImage> {
    let mut buffer = image.into_buffer();
    let m = params.target.acescg_matrix();
    pixels::map_in_place(buffer.rgb_mut(), |px| {
        let [r, g, b] = *px;
        *px = [
            m[0][0] * r + m[0][1] * g + m[0][2] * b,
            m[1][0] * r + m[1][1] * g + m[1][2] * b,
            m[2][0] * r + m[2][1] * g + m[2][2] * b,
        ];
    });
    Ok(DisplayReferredImage(buffer, params.target))
}
