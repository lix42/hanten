//! Pure pipeline stages between decode and encode: film-base estimation, color
//! transforms, and the stage wiring that threads them together.
//!
//! The rendering chain is [`chain`], composing [`scene_correction`] → [`look`] →
//! [`fit_range`] → [`fit_gamut`] over the shared buffer in [`working_image`]. The fixed
//! decode (`algo::fixed`) feeds it through the NC film RGB v1 mapping
//! ([`working_space`]), and it renders into the destination `crate::destination`
//! resolves; the film master skips it. Scene correction applies white balance and
//! exposure, the look contrast, the per-channel grade and highlight desaturation, fit
//! range compresses the scene's range against the destination's peak and places black,
//! and fit gamut maps into the destination's gamut; the stage epics fill the rest.
//! [`white_balance`] holds the white-balance statistics the roll measurement
//! ([`roll_white`]) pools.

/// The SDR/HDR branch contract on real frames (`nf-display-stages/branch-contract`).
#[cfg(test)]
mod branch_probe;
pub mod chain;
/// Goldens for the chain's stages (`nf-verification/stage-goldens`).
#[cfg(test)]
mod chain_golden;
pub mod color;
pub mod colorimetry;
pub mod correction_confidence;
pub mod film_base;
pub mod fit_gamut;
pub mod fit_range;
pub mod gain_encode;
pub mod gain_ratio;
pub mod hdr;
pub mod input_semantics;
pub mod look;
pub mod memory;
pub mod midtone_neutral;
pub mod pixels;
pub mod roll_white;
pub mod scene_correction;
/// Test-only diagnostic harness from `algo/reference-anchored-sigmoid`. `cfg(test)` so it
/// never reaches the shipped binary; its asset-dependent entries are `#[ignore]`d.
#[cfg(test)]
pub mod shadow_metrics;
pub mod white_balance;
pub mod working_image;
pub mod working_space;
