//! Negative reconstruction (design-spec §7): the fixed decode ([`fixed`]), which returns
//! the typed [`FilmRgbImage`] boundary:
//!
//! ```text
//! scan → Dmin normalization → corrected density D′ → the straight line → FilmRgbImage
//! ```
//!
//! [`FilmRgbImage`]'s fields are private and its only constructor is
//! `pub(in crate::algo)`, so the decode inside this module tree is its only producer —
//! downstream stages that accept a `FilmRgbImage` (the NC-film-RGB → ACEScg
//! working-space mapper) can never be handed a raw scan or density buffer.

pub mod fixed;

use crate::types::LinearImage;

/// The typed film-rendering RGB boundary every reconstruction path produces:
/// the unclamped linear positive in NC's film-rendering interpretation. It has no
/// IR plane: the decode drops it ([`fixed::decode`]). Fields are **private** and the constructor is
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
}

impl FilmRgbImage {
    /// The shipped constructor — restricted to the `algo` module tree (note:
    /// `pub(super)` would NOT do this: `algo` is a top-level module, so its
    /// `super` is the crate root and `pub(super)` would be crate-wide), so
    /// the fixed decode ([`fixed::decode`]) is the only producer outside tests (see
    /// `FilmRgbImage::fixture` (test-only)). Takes an
    /// already-validated [`LinearImage`] so the buffer length invariants hold
    /// by construction. An IR plane on `image` is dropped.
    pub(in crate::algo) fn from_linear(image: LinearImage) -> Self {
        Self {
            width: image.width,
            height: image.height,
            rgb: image.rgb,
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

    // The read accessors below are the boundary's inspection API. Rendering takes the
    // whole image across the boundary (`into_linear`, the working-space mapper);
    // `measure-roll` reads the dimensions and pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Read-only view of the interleaved film positive. Rendering takes the whole image
    /// across the boundary instead; `measure-roll` reads it to measure a frame's white in
    /// film RGB, where it was reviewed (`pipeline::roll_white::sample_white`).
    pub fn rgb(&self) -> &[f32] {
        &self.rgb
    }

    /// Unwrap into the plain working-space image type — the **read** direction
    /// of the boundary, used by the working-space mapper. Constructing a
    /// `FilmRgbImage` stays restricted; reading one out is not the invariant the
    /// type protects.
    pub(crate) fn into_linear(self) -> LinearImage {
        // The fields came from a validated LinearImage and are never resized,
        // so the invariants hold; route through the validated constructor
        // anyway (its checks are O(1)) so a future regression fails loudly.
        LinearImage::new(self.width, self.height, self.rgb, None)
            .expect("FilmRgbImage preserves the validated buffer-length invariants")
    }
}

impl std::fmt::Debug for FilmRgbImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilmRgbImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FilmBase;

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

    #[test]
    fn reconstruction_returns_a_film_rgb_image() {
        // The type-level boundary: the decode produces a `FilmRgbImage` (enforced by
        // its signature) with the dimensions intact. It has no IR field.
        let (film, _) = fixed::decode(image(), &base(), &fixed::DecodeParams::default()).unwrap();
        assert_eq!((film.width(), film.height()), (2, 1));
        assert_eq!(film.rgb().len(), 6);
        let linear = film.into_linear();
        assert_eq!((linear.width, linear.height), (2, 1));
    }

    // `FilmRgbImage`'s construction privacy is enforced by the compiler:
    // `from_linear` is `pub(in crate::algo)`, so no code outside the `algo`
    // module tree can mint one — the working-space mapper can only receive
    // what the decode produced. (A compile-fail test would need a
    // `trybuild` dev-dependency; the privacy annotation is the guarantee.)
}
