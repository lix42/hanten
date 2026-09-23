//! **Stage 3 of the new rendering chain — fit range.** Identity for now.
//!
//! Fit the scene's dynamic range into the display's, with the display's peak as
//! the stage's parameter — which is what makes SDR and HDR the same function with
//! different arguments rather than two renderers. The industry term is *tone
//! mapping*; "tone" there means brightness levels, not colour.
//! `nf-display-stages/fit-range` fills it, with reinhard as the baseline setting.
//!
//! Written fresh. `pipeline::sdr::render_pixel` and its HDR counterpart each fuse
//! tone selection, the luminance rescale and the gamut map into one loop body,
//! once per branch; per CLAUDE.md's migration rule the stage is not extracted out
//! of those bodies because that is where the arithmetic happens to live today.
//!
//! **This is where a toe belongs**, not in reconstruction: shaping the approach to
//! black needs the display's range, and reconstruction does not know it. Whether
//! the operator gains an explicit one is `nf-display-stages/parametric-operator`'s
//! question.

use std::fmt;

use crate::pipeline::look::GradedImage;
use crate::pipeline::working_image::WorkingBuffer;
use crate::types::Result;

/// Fit range's knobs. Empty until `nf-display-stages/fit-range` gives it an
/// operator and the display's peak. See [`SceneCorrectionParams`] on why this is
/// an empty struct rather than an `Option`.
///
/// [`SceneCorrectionParams`]: crate::pipeline::scene_correction::SceneCorrectionParams
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitRangeParams {}

impl FitRangeParams {
    /// What fit range does under these parameters, for the report — stated by the
    /// stage rather than by its caller, so filling the stage changes the report in the
    /// same edit. `"identity"` until the stage has knobs.
    pub fn applied(&self) -> &'static str {
        "identity"
    }
}

/// Pixels whose **luminance** now fits the display's range, before the gamut is
/// fitted.
///
/// A separate boundary from [`DisplayReferredImage`] because the two stages are
/// coupled but distinct: today's SDR gamut ceiling follows the range the tone
/// produced, which is why they stay adjacent rather than merged.
///
/// [`DisplayReferredImage`]: crate::pipeline::fit_gamut::DisplayReferredImage
pub struct RangeFittedImage(WorkingBuffer);

impl RangeFittedImage {
    /// Hand the buffer to the next stage. Consuming, so the pixels move rather
    /// than copy.
    pub(in crate::pipeline) fn into_buffer(self) -> WorkingBuffer {
        self.0
    }
}

impl fmt::Debug for RangeFittedImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_named(f, "RangeFittedImage")
    }
}

/// Fit the scene's range to the display's — **identity today**, and bit-exactly so.
///
/// Fallible by construction, not by need: see [`chain::render`]. This stage is the
/// clearest case for it — its legacy counterparts `pipeline::sdr::render` and
/// `pipeline::hdr::render_linear` both refuse a non-finite sample.
///
/// [`chain::render`]: crate::pipeline::chain::render
pub fn apply(image: GradedImage, _params: &FitRangeParams) -> Result<RangeFittedImage> {
    Ok(RangeFittedImage(image.into_buffer()))
}
