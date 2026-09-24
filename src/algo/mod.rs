//! Negative reconstruction and density curves (design-spec §7).
//!
//! The [`Reconstruction`] config drives the current chain's one path —
//! Dmin-normalized corrected density `D′` mapped through a tagged exponential or
//! characteristic curve — and the new chain's fixed decode ([`fixed`]) is a second
//! producer. **Both return the typed [`FilmRgbImage`] boundary**:
//!
//! ```text
//! scan → Dmin normalization → corrected density D′   (density reconstruction)
//!      → exponential | characteristic density curve   (the curve stage)
//!      → FilmRgbImage                                  (typed boundary)
//! ```
//!
//! [`FilmRgbImage`]'s fields are private and its only constructor is
//! `pub(in crate::algo)`, so [`reconstruct`]'s paths inside this module tree
//! are the only producers — downstream stages that accept a `FilmRgbImage`
//! (the NC-film-RGB → ACEScg working-space mapper) can never be handed a raw
//! scan or density buffer. The pixel arithmetic of [`reconstruct`] is
//! bit-identical to the pre-split monolithic converters' reconstruction half
//! (pinned by the golden fixtures in `pipeline::stages`, `mod golden`).

pub mod characteristic;
pub mod density;
pub mod fixed;

/// The probe that measured whether inverting the published curves removes the per-channel
/// cast a single scalar contrast leaves. Test-only, asset-gated, prints derived numbers
/// only — see its header for the method and `docs/progress/algo.md` for the result.
#[cfg(test)]
mod curve_probe;

use crate::types::{FilmBase, LinearImage, Reconstruction, Result};

/// The typed film-rendering RGB boundary every reconstruction path produces:
/// the unclamped linear positive in NC's film-rendering interpretation, plus
/// the carried-through IR plane. Fields are **private** and the constructor is
/// `pub(in crate::algo)`, so only the `algo` module tree's reconstruction
/// paths can mint one — a raw scan or density buffer cannot impersonate film
/// RGB downstream (the working-space mapper accepts `FilmRgbImage`, nothing
/// else). The one exception is the test-only `FilmRgbImage::fixture`, which
/// lets a test place chosen values at the mapper's input.
///
/// Values are deliberately unclamped (HDR/scene-headroom preserved; range
/// clamping happens only at the u16 encode step) and may be non-finite when
/// the input was (fail-loud propagation to `io::encode`'s counters).
///
/// `Debug` prints only the dimensions (never the pixel buffers) — it exists so
/// `Result<FilmRgbImage, _>` works with `unwrap_err`/`expect` in tests.
pub struct FilmRgbImage {
    width: u32,
    height: u32,
    /// Interleaved `r,g,b` positive, `len == width * height * 3`.
    rgb: Vec<f32>,
    /// Carried-through IR plane (HDRi input), `len == width * height`.
    ir: Option<Vec<f32>>,
}

impl FilmRgbImage {
    /// The shipped constructor — restricted to the `algo` module tree (note:
    /// `pub(super)` would NOT do this: `algo` is a top-level module, so its
    /// `super` is the crate root and `pub(super)` would be crate-wide), so
    /// [`reconstruct`]'s paths are the only producers outside tests (see
    /// `FilmRgbImage::fixture` (test-only)). Takes an
    /// already-validated [`LinearImage`] so the buffer length invariants hold
    /// by construction.
    pub(in crate::algo) fn from_linear(image: LinearImage) -> Self {
        Self {
            width: image.width,
            height: image.height,
            rgb: image.rgb,
            ir: image.ir,
        }
    }

    /// **Test fixture**: a film positive holding exactly `image`'s values, so a test
    /// can place a chosen value — non-finite ones included — at the working-space
    /// mapper's input without running a reconstruction.
    ///
    /// The one fixture for "a `FilmRgbImage` a test is not about" — use it rather than
    /// growing a module-local producer.
    #[cfg(test)]
    pub(crate) fn fixture(image: LinearImage) -> Self {
        Self::from_linear(image)
    }

    // The read accessors below are the boundary's inspection API, exercised only by
    // tests: every production consumer takes the whole image across the boundary
    // (`into_linear`, the working-space mapper) — a narrow documented allow per the
    // house rule.
    #[allow(dead_code)]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[allow(dead_code)]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Read-only view of the interleaved film positive. Test-only: every production
    /// consumer takes the whole image across the boundary instead.
    #[cfg(test)]
    pub fn rgb(&self) -> &[f32] {
        &self.rgb
    }

    /// Read-only view of the carried IR plane, when the input had one.
    #[allow(dead_code)]
    pub fn ir(&self) -> Option<&[f32]> {
        self.ir.as_deref()
    }

    /// Unwrap into the plain working-space image type — the **read** direction
    /// of the boundary, used by the working-space mapper. Constructing a
    /// `FilmRgbImage` stays restricted; reading one out is not the invariant the
    /// type protects.
    pub(crate) fn into_linear(self) -> LinearImage {
        // The fields came from a validated LinearImage and are never resized,
        // so the invariants hold; route through the validated constructor
        // anyway (its checks are O(1)) so a future regression fails loudly.
        LinearImage::new(self.width, self.height, self.rgb, self.ir)
            .expect("FilmRgbImage preserves the validated buffer-length invariants")
    }
}

impl std::fmt::Debug for FilmRgbImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilmRgbImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("ir", &self.ir.is_some())
            .finish_non_exhaustive()
    }
}

/// Diagnostics the reconstruction stage surfaces for the JSON report — the
/// resolved values, not new knobs (controls live in [`Reconstruction`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReconstructionReport {
    /// The **derived** anchor the curve used — the corrected density that rendered to
    /// `1.0`, and therefore what sets the black floor at `10^(−contrast·anchor)`.
    /// `None` for the characteristic curve, which places no anchor.
    pub curve_anchor: Option<f32>,
    /// The resolved regional-balance tone-ramp range `[lo, hi]` (corrected
    /// density), when a shadow/highlight balance was applied. `None` when both
    /// balances are the neutral `[0, 0, 0]`.
    pub balance_range: Option<[f32; 2]>,
    /// How far the frame's densities fell outside the stock's published curve, per channel
    /// — `Some` only for the characteristic curve, `None` for every other path.
    ///
    /// Reported rather than clamped. Out-of-table samples extrapolate along the end slope,
    /// which keeps them ordered and finite, but they are **extrapolated**, not measured:
    /// a frame with a large fraction of them is being rendered off the published data, and
    /// the report has to say so instead of leaving it to be inferred from the picture.
    pub out_of_table: Option<characteristic::OutOfTable>,
}

/// Stage 3 — reconstruct the negative into the typed film positive
/// (design-spec §7): pure `(input, config) -> output`. The IR plane is carried
/// through untouched (Step-1 rule: preserve, don't consume). Total in its inputs: a
/// degenerate film base or an unusable curve anchor surfaces as an
/// [`NcError`](crate::types::NcError), never a silently-wrong image.
pub fn reconstruct(
    image: &LinearImage,
    base: &FilmBase,
    config: &Reconstruction,
) -> Result<(FilmRgbImage, ReconstructionReport)> {
    density::reconstruct(image, base, &config.density, &config.curve)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CharacteristicParams, DensityCurve, DensityParams};

    fn image() -> LinearImage {
        LinearImage::new(
            2,
            1,
            vec![0.5, 0.3, 0.2, 0.05, 0.03, 0.02],
            Some(vec![0.25, 0.75]),
        )
        .unwrap()
    }

    fn base() -> FilmBase {
        FilmBase::from([0.9, 0.55, 0.42])
    }

    /// Every supported curve, for exhaustive path checks.
    fn all_configs() -> [Reconstruction; 2] {
        [
            Reconstruction::default(),
            Reconstruction {
                density: DensityParams::default(),
                curve: DensityCurve::Characteristic(CharacteristicParams::default()),
            },
        ]
    }

    #[test]
    fn every_path_returns_a_film_rgb_image_and_preserves_ir() {
        // The type-level boundary: each supported config produces a
        // `FilmRgbImage` (enforced by `reconstruct`'s signature — this test
        // exercises all paths) with the dimensions and IR plane intact.
        for config in all_configs() {
            let (film, _) = reconstruct(&image(), &base(), &config).unwrap();
            assert_eq!((film.width(), film.height()), (2, 1), "{config:?}");
            assert_eq!(film.rgb().len(), 6, "{config:?}");
            assert_eq!(film.ir(), Some(&[0.25_f32, 0.75][..]), "{config:?}");
            // The read direction round-trips losslessly.
            let linear = film.into_linear();
            assert_eq!((linear.width, linear.height), (2, 1));
            assert_eq!(linear.ir.as_deref(), Some(&[0.25_f32, 0.75][..]));
        }
    }

    // `FilmRgbImage`'s construction privacy is enforced by the compiler:
    // `from_linear` is `pub(in crate::algo)`, so no code outside the `algo`
    // module tree can mint one — the working-space mapper can only receive
    // what `reconstruct` produced. (A compile-fail test would need a
    // `trybuild` dev-dependency; the privacy annotation is the guarantee.)

    #[test]
    fn the_default_reports_its_base_derived_anchor() {
        let (_, report) = reconstruct(&image(), &base(), &Reconstruction::default()).unwrap();
        let expected =
            fixed::MID_ABOVE_BASE + crate::types::MID_GREY_OUTPUT_DECADES / fixed::CONTRAST;
        assert_eq!(report.curve_anchor, Some(expected));
        assert_eq!(report.balance_range, None);
    }
}
