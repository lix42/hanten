//! Pure pipeline stages between decode and encode: film-base estimation, color
//! transforms, and the stage wiring that threads them together.
//!
//! **Two chains live here during the migration** (`docs/nf-migration.md`). The
//! shipped one runs `stages::render_display_source` (`render_split`) → `sdr`/`hdr`,
//! or `stages::render_film_master` for the master; the one
//! `--new-flow` selects is [`chain`], composing [`scene_correction`] → [`look`] →
//! [`fit_range`] → [`fit_gamut`] over the shared buffer in [`working_image`].
//! The new stages are named for the job they do rather than for the migration, so
//! that retiring the old path is a deletion and not a rename.
//!
//! `--new-flow` reaches the new chain (`nf-core/minimal-end-to-end`): the fixed decode
//! (`algo::fixed`) feeds it, and it renders into one destination, a Display P3 16-bit
//! TIFF. Scene correction applies white balance and exposure, the look and fit range
//! are identity passes, and fit gamut applies only the change of primaries; the stage
//! epics fill the rest. [`white_balance`] holds the auto estimators both chains use.

pub mod chain;
/// Goldens for the new flow's stages (`nf-verification/stage-goldens`).
#[cfg(test)]
mod chain_golden;
pub mod color;
pub mod colorimetry;
pub mod display_tone;
pub mod film_base;
pub mod fit_gamut;
pub mod fit_range;
pub mod gain_map;
pub mod hdr;
pub mod input_semantics;
pub mod look;
pub mod memory;
pub mod pixels;
pub mod render_split;
pub mod scene_correction;
pub mod sdr;
/// Test-only diagnostic harness from `algo/reference-anchored-sigmoid`. `cfg(test)` so it
/// never reaches the shipped binary; its asset-dependent entries are `#[ignore]`d.
#[cfg(test)]
pub mod shadow_metrics;
pub mod stages;
pub mod white_balance;
pub mod working_image;
pub mod working_space;
