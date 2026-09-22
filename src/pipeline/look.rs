//! **Stage 2 of the new rendering chain — the look.** Identity for now.
//!
//! The creative stage nc does not have today: contrast, the per-channel grade with
//! a mid-grey pivot, highlight desaturation, and later print emulation and
//! per-stock normalization. Scene-referred and linear. `nf-look/stage` fills it;
//! the control tasks under `nf-look` land in it.
//!
//! **Its position is a constraint, not a preference.** The look sits after scene
//! correction and *above* the SDR/HDR branch, because a gain map requires the two
//! renditions to agree below diffuse white — so anything shaping midtone character
//! must be applied once, before the split (`docs/design-update.md` Part 2). Only
//! fit range and later may differ per branch.

use std::fmt;

use crate::pipeline::scene_correction::SceneReferredImage;
use crate::pipeline::working_image::WorkingBuffer;
use crate::types::Result;

/// The look's knobs. Empty until `nf-look/stage` gives it a recipe section and
/// the three controls land in it. See [`SceneCorrectionParams`] on why this is an
/// empty struct rather than an `Option`.
///
/// [`SceneCorrectionParams`]: crate::pipeline::scene_correction::SceneCorrectionParams
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookParams {}

impl LookParams {
    /// What the look does under these parameters, for the report — stated by the
    /// stage rather than by its caller, so filling the stage changes the report in the
    /// same edit. `"identity"` until the stage has knobs.
    pub fn applied(&self) -> &'static str {
        "identity"
    }
}

/// Linear ACEScg after the look: scene-referred, graded.
///
/// This is also **the one source both display branches share**. The branch point
/// is below it (`nf-display-stages/branch-contract`), so whatever that task
/// decides about the split, this is the boundary it splits *from*.
pub struct GradedImage(WorkingBuffer);

impl GradedImage {
    /// Hand the buffer to the next stage. Consuming, so the pixels move rather
    /// than copy.
    pub(in crate::pipeline) fn into_buffer(self) -> WorkingBuffer {
        self.0
    }
}

impl fmt::Debug for GradedImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_named(f, "GradedImage")
    }
}

/// Apply the look — **identity today**, and bit-exactly so.
///
/// Fallible by construction, not by need: see [`chain::render`].
///
/// [`chain::render`]: crate::pipeline::chain::render
pub fn apply(image: SceneReferredImage, _params: &LookParams) -> Result<GradedImage> {
    Ok(GradedImage(image.into_buffer()))
}
