//! **Stage 1 of the new rendering chain — scene correction.** Identity for now.
//!
//! Photographic corrections toward what the scene was: white balance, exposure,
//! and later the flare/fog half of the black point. Scene-referred and linear.
//! `nf-scene-correction/stage` fills it; `nf-scene-correction/{flare-removal,
//! levels-knob}` add to it.
//!
//! Written fresh, per CLAUDE.md's migration rule. `render_split::apply_shared_controls`
//! fuses WB → exposure → black point → `linear_range` into one loop body and is worth
//! reading for *what the arithmetic does*, but it is not the template for this
//! boundary.
//!
//! Note the corrections here act on working-space channels, **after** the NC film
//! RGB v1 3×3 — so this white balance is not reconstruction's `offset`, and this
//! exposure is not its anchor (`docs/design-update.md` Part 1 measures the two
//! white-balance bases ≈2.6 % apart on a neutral).

use std::fmt;

use crate::pipeline::working_image::WorkingBuffer;
use crate::pipeline::working_space::AcesCgImage;
use crate::types::Result;

/// Scene correction's knobs. Empty until `nf-scene-correction/stage` gives it
/// white balance and exposure.
///
/// An empty struct rather than an `Option`: a stage is always in the chain, and
/// "this stage is off" is deliberately not expressible — `nf-look/stage.md`
/// settled that an empty stage reports as empty, not absent. Filling the stage
/// adds fields here; [`apply`]'s signature already accommodates failure (see
/// [`chain::render`]). How a filled stage hands its *resolved* values back for the
/// report is not settled here — that is `nf-core/report-contract`'s.
///
/// [`chain::render`]: crate::pipeline::chain::render
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneCorrectionParams {}

impl SceneCorrectionParams {
    /// What scene correction does under these parameters, for the report — stated by the
    /// stage rather than by its caller, so filling the stage changes the report in the
    /// same edit. `"identity"` until the stage has knobs.
    pub fn applied(&self) -> &'static str {
        "identity"
    }
}

/// Linear ACEScg after scene correction: still **scene-referred**, now carrying
/// the photographic corrections.
///
/// The field is private to this module, so [`apply`] is the only function that
/// can mint one — the [`AcesCgImage`] pattern, and what keeps a buffer that
/// skipped the stage out of the next one.
pub struct SceneReferredImage(WorkingBuffer);

impl SceneReferredImage {
    /// Hand the buffer to the next stage. Consuming, so the pixels move rather
    /// than copy.
    pub(in crate::pipeline) fn into_buffer(self) -> WorkingBuffer {
        self.0
    }
}

impl fmt::Debug for SceneReferredImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_named(f, "SceneReferredImage")
    }
}

/// Apply scene correction — **identity today**, and bit-exactly so.
///
/// Fallible by construction, not by need: see [`chain::render`].
///
/// [`chain::render`]: crate::pipeline::chain::render
pub fn apply(image: AcesCgImage, _params: &SceneCorrectionParams) -> Result<SceneReferredImage> {
    Ok(SceneReferredImage(WorkingBuffer::from_aces(image)))
}
