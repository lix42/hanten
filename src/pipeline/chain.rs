//! The new rendering chain, composed: scene correction → look → fit range → fit
//! gamut.
//!
//! The chain `--new-flow` selects (`docs/design-update.md` Part 2,
//! `docs/nf-migration.md`), fed by the fixed decode (`algo::fixed`) and rendering
//! into one destination (`cli::convert_frame`, `nf-core/minimal-end-to-end`). Scene
//! correction applies white balance and exposure; the look is still an **identity
//! pass** — its epic fills it — fit range compresses the scene's range against the
//! destination's peak, and fit gamut applies only the change of primaries into the
//! destination's gamut.
//!
//! **The order is carried by the types, not by this function.** Each stage's
//! input is the previous stage's output type, and each of those can be minted
//! only inside the module that produces it — so a chain composed out of order
//! does not compile. That is the whole point of the skeleton: the boundaries are
//! decided once, deliberately, rather than falling out of whichever stage ships
//! first.
//!
//! Entering the chain, crossing each boundary, and **leaving** it
//! (`DisplayReferredImage::into_parts`) all move the pixel buffers, so a type per
//! stage costs no allocation. The exit is the one worth stating separately: the
//! encoder takes `&LinearImage`, so a boundary with only `&`-accessors would force
//! a full-frame copy at the hand-off — ~0.9 GB on a 74.6 MP scan — and the "no
//! allocation" claim would be false at exactly the point it matters most. What each
//! stage then *does* with the buffer it owns is `nf-core/buffer-strategy`'s to
//! settle.

use crate::pipeline::fit_gamut::{self, DisplayReferredImage, FitGamutParams};
use crate::pipeline::fit_range::{self, FitRange, FitRangeParams};
use crate::pipeline::look::{self, LookParams};
use crate::pipeline::scene_correction::{self, SceneCorrection, SceneCorrectionParams};
use crate::pipeline::working_space::AcesCgImage;
use crate::types::Result;

/// Every stage's parameters, in chain order.
///
/// Not a recipe type, though two of its four fields are: `scene_correction` and `look`
/// are top-level sections of the new chain's recipe (`crate::recipe::Recipe`) as they
/// stand, while fit range's peak and fit gamut's target are the destination's, so the
/// recipe's `fit_range` and `fit_gamut` sections are their own types and
/// [`crate::recipe::Recipe::chain_params`] adds the destination's half. These structs
/// exist so a stage's signature is settled now and does not change when its knobs
/// arrive. No `Default`, because neither [`FitRangeParams`] nor [`FitGamutParams`] has
/// one: the destination states its peak and its gamut.
#[derive(Clone, Debug, PartialEq)]
pub struct ChainParams {
    pub scene_correction: SceneCorrectionParams,
    pub look: LookParams,
    pub fit_range: FitRangeParams,
    pub fit_gamut: FitGamutParams,
}

/// What [`render`] produced: the display-referred image, and what the chain applied
/// to reach it.
pub struct Rendered {
    pub image: DisplayReferredImage,
    /// Each stage, in the order `render` ran it, with what it applied — the report's
    /// account of the chain. Built inside `render` so a stage inserted, moved or
    /// renamed there cannot leave the report listing the old chain, and read off the
    /// values each stage applied, so an operation that moved no pixel is reported as
    /// `"identity"`.
    pub applied: [(&'static str, &'static str); 4],
    /// Scene correction's values as applied to this frame.
    pub scene_correction: SceneCorrection,
    /// Fit range's operator and its arguments, as resolved.
    pub fit_range: FitRange,
}

/// Render an [`AcesCgImage`] through the new chain.
///
/// **Today this is scene correction's per-channel gains, fit range's luminance
/// operator and the destination's 3×3** — the look is an identity, and fit gamut
/// applies only the change of primaries. Nothing is clamped: values outside `[0, 1]`
/// ride through to the encoder, which is the only place clamping happens. A
/// non-finite sample is **refused** by fit range, naming the pixel.
///
/// **Fallible by construction.** Every stage's signature returns a `Result` and so
/// does this, although only scene correction and fit range can fail yet. That is the
/// point of settling the boundaries once: every stage this chain will host has a
/// *fallible* counterpart in the shipped code — `render_split::display_source`,
/// `sdr::render` (which errors on a non-finite sample) and `hdr::render_linear` all
/// return `Result` — so a stage that gains its arithmetic would otherwise change its
/// signature, this function's, every call site and every test here. The cost for a
/// stage that cannot fail is an `Ok` wrapper.
///
/// The single-branch shape is deliberate. The SDR/HDR split belongs below the
/// look — a gain map requires the two renditions to agree below diffuse white —
/// but where exactly, and whether a single-rendition destination goes through the
/// branch point at all, is `nf-display-stages/branch-contract`'s open question.
/// It splits *from* [`GradedImage`], whichever way it lands.
///
/// [`GradedImage`]: crate::pipeline::look::GradedImage
pub fn render(image: AcesCgImage, params: &ChainParams) -> Result<Rendered> {
    let (corrected, scene_correction) = scene_correction::apply(image, &params.scene_correction)?;
    let graded = look::apply(corrected, &params.look)?;
    let fitted = fit_range::apply(graded, &params.fit_range)?;
    let image = fit_gamut::apply(fitted, &params.fit_gamut)?;
    let fit_range = params.fit_range.resolved();
    Ok(Rendered {
        image,
        applied: [
            ("scene_correction", scene_correction.applied()),
            ("look", params.look.applied()),
            ("fit_range", fit_range.operator),
            ("fit_gamut", params.fit_gamut.applied()),
        ],
        scene_correction,
        fit_range,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::{FilmRgbImage, reconstruct};
    use crate::pipeline::colorimetry::pinned::ACESCG_TO_DISPLAY_P3;
    use crate::pipeline::fit_gamut::DestinationGamut;
    use crate::pipeline::fit_range::{DisplayPeak, RangeFittedImage};
    use crate::pipeline::scene_correction::WhiteBalance;
    use crate::pipeline::working_space::map_nc_film_rgb_v1;
    use crate::types::{
        CharacteristicParams, DensityCurve, DensityParams, ExponentialParams, FilmBase,
        LinearImage, Reconstruction,
    };

    /// Every stage at its identity — fit range at zero headroom — so a test sees the
    /// wiring and the destination matrix rather than the operator.
    fn params() -> ChainParams {
        ChainParams {
            scene_correction: SceneCorrectionParams::default(),
            look: LookParams::default(),
            fit_range: FitRangeParams {
                headroom_stops: 0.0,
                peak: DisplayPeak::SDR,
            },
            fit_gamut: FitGamutParams {
                target: DestinationGamut::DisplayP3,
            },
        }
    }

    /// [`params`] with fit range at the recipe's default headroom — what a run gets.
    fn shipped_params() -> ChainParams {
        let mut p = params();
        p.fit_range.headroom_stops = crate::types::DEFAULT_HEADROOM_STOPS;
        p
    }

    /// An `AcesCgImage` whose *film RGB* input was exactly `rgb` — including a
    /// non-finite value — through the real working-space mapper.
    fn aces_from(width: u32, height: u32, rgb: &[f32], ir: Option<Vec<f32>>) -> AcesCgImage {
        let film =
            FilmRgbImage::fixture(LinearImage::new(width, height, rgb.to_vec(), ir).unwrap());
        map_nc_film_rgb_v1(film)
    }

    /// Bit patterns, not values: `NaN != NaN`, so an `==` comparison would pass
    /// over exactly the samples this asserts survive untouched.
    fn bits(pixels: &[f32]) -> Vec<u32> {
        pixels.iter().map(|v| v.to_bits()).collect()
    }

    /// The chain up to fit range with every stage at its identity.
    fn through_fit_range(image: AcesCgImage) -> RangeFittedImage {
        let p = params();
        let corrected = scene_correction::apply(image, &p.scene_correction)
            .unwrap()
            .0;
        let graded = look::apply(corrected, &p.look).unwrap();
        fit_range::apply(graded, &p.fit_range).unwrap()
    }

    /// The expected output of fit gamut, written out independently of the stage:
    /// the pinned matrix, row by row, in the same order of operations.
    fn to_p3(rgb: &[f32]) -> Vec<f32> {
        let m = ACESCG_TO_DISPLAY_P3;
        rgb.as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| {
                [
                    m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2],
                    m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2],
                    m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2],
                ]
            })
            .collect()
    }

    /// Ordinary values, both ends of the working range, and the awkward finite ones:
    /// above 1.0 (unclamped is the contract), below 0.0 (a wide-gamut linear space
    /// contains them), and far past any white point.
    const FINITE: [f32; 9] = [0.0, 0.18, 1.0, 5.0, -0.25, 1e6, 0.5, 0.25, -1e-3];

    /// [`FINITE`]'s shape with non-finite samples in pixels 1 and 2, which fit range
    /// refuses.
    const AWKWARD: [f32; 9] = [
        0.0,
        0.18,
        1.0,
        5.0,
        -0.25,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        0.5,
    ];

    #[test]
    fn the_first_three_stages_are_a_bit_exact_identity() {
        let aces = aces_from(3, 1, &FINITE, None);
        let before = bits(aces.rgb());

        let out = through_fit_range(aces).into_buffer().into_linear();

        assert_eq!(bits(&out.rgb), before, "a stage moved a pixel");
    }

    #[test]
    fn a_one_ulp_move_in_the_rendered_output_is_visible() {
        // Falsifiability control for the identity test above, which is evidence only
        // if its comparison can fail on the values this chain actually carries. The
        // comparison is against the **unperturbed output**, not against the input:
        // asserting against the input would also pass on any chain that stops being
        // an identity, i.e. exactly when this control stops exercising ULP detection.
        // Making a stage non-identity needs a deliberately broken build, done by hand
        // when the chain landed (`docs/progress/nf-core.md`).
        let aces = aces_from(
            3,
            1,
            &[0.0, 0.18, 1.0, 5.0, -0.25, 0.5, 0.25, 0.75, 0.9],
            None,
        );
        let rendered = bits(&through_fit_range(aces).into_buffer().into_linear().rgb);
        let mut moved = rendered.clone();

        // No NaN in this vector, so `next_up` genuinely steps.
        let sample = f32::from_bits(moved[4]);
        assert!(sample.is_finite(), "the perturbed sample must be finite");
        moved[4] = sample.next_up().to_bits();

        assert_ne!(
            moved, rendered,
            "a one-ULP move must be visible to the compare"
        );
    }

    #[test]
    fn the_chain_applies_the_destination_matrix_and_nothing_else() {
        // What the whole chain does with every stage at its identity: the pinned
        // ACEScg → Display P3 3×3, bit for bit, with nothing clamped.
        let aces = aces_from(3, 1, &FINITE, None);
        let expected = bits(&to_p3(aces.rgb()));
        let (out, gamut) = render(aces, &params()).unwrap().image.into_parts();
        assert_eq!(gamut, DestinationGamut::DisplayP3);
        assert_eq!(bits(&out.rgb), expected);
    }

    #[test]
    fn a_non_finite_sample_is_refused_naming_the_first_pixel() {
        // At the identity headroom too: whether a frame renders must not depend on
        // the setting.
        for p in [params(), shipped_params()] {
            let aces = aces_from(3, 1, &AWKWARD, None);
            let err = render(aces, &p).err().expect("a non-finite sample");
            let msg = err.message();
            assert!(
                msg.contains("fit range") && msg.contains("pixel 1"),
                "{msg}"
            );
        }
    }

    #[test]
    fn the_shipped_fit_range_compresses_highlights_and_keeps_mid_grey_and_hue() {
        let aces = aces_from(
            3,
            1,
            &[0.18, 0.18, 0.18, 4.0, 4.0, 4.0, 3.0, 1.5, 0.5],
            None,
        );
        let input = aces.rgb().to_vec();
        let p = shipped_params();
        let corrected = scene_correction::apply(aces, &p.scene_correction)
            .unwrap()
            .0;
        let graded = look::apply(corrected, &p.look).unwrap();
        let out = fit_range::apply(graded, &p.fit_range)
            .unwrap()
            .into_buffer()
            .into_linear()
            .rgb;
        for c in 0..3 {
            assert!((out[c] - input[c]).abs() < 1e-5, "mid-grey moved: {out:?}");
            assert!(out[3 + c] < 0.8 * input[3 + c], "not compressed: {out:?}");
        }
        // One scale for all three channels: the ratios survive.
        for c in 0..3 {
            let ratio = out[6 + c] / input[6 + c];
            assert!(
                (ratio - out[6] / input[6]).abs() < 1e-5,
                "hue moved: {out:?}"
            );
        }
        let rendered = render(aces_from(1, 1, &[0.5, 0.5, 0.5], None), &p).unwrap();
        assert_eq!(rendered.applied[2], ("fit_range", fit_range::OPERATOR));
        assert_eq!(rendered.fit_range.display_peak, DisplayPeak::SDR);
    }

    #[test]
    fn the_chain_leaves_both_sides_of_the_cube_unclamped() {
        // Finite values only, so the assertions below cannot be satisfied by an
        // infinity: a bright neutral lands above 1.0, and a colour outside the film-RGB
        // cube (which an unclamped decode can produce) lands outside P3 with a negative
        // channel. Clamping is the encoder's alone.
        let aces = aces_from(2, 1, &[4.0, 4.0, 4.0, 1.0, -0.5, -0.5], None);
        let (out, _) = render(aces, &params()).unwrap().image.into_parts();

        assert!(out.rgb.iter().all(|v| v.is_finite()));
        assert!(out.rgb[..3].iter().all(|v| *v > 1.0), "{:?}", &out.rgb[..3]);
        assert!(out.rgb[3..].iter().any(|v| *v < 0.0), "{:?}", &out.rgb[3..]);
    }

    #[test]
    fn a_neutral_stays_neutral_through_the_destination_matrix() {
        // The adapted matrix maps the ACES white to D65 white, so an ACEScg neutral
        // lands on the P3 neutral axis. Tolerance, not bits: the rows sum to 1 only to
        // f32 rounding.
        let aces = aces_from(1, 1, &[0.18, 0.18, 0.18], None);
        let neutral = aces.rgb().to_vec();
        assert!(
            (neutral[0] - neutral[1]).abs() < 1e-6 && (neutral[1] - neutral[2]).abs() < 1e-6,
            "the film-RGB neutral must reach the chain as an ACEScg neutral: {neutral:?}"
        );

        let (out, _) = render(aces, &params()).unwrap().image.into_parts();
        for c in 0..3 {
            assert!(
                (out.rgb[c] - neutral[0]).abs() < 1e-5,
                "channel {c}: {} vs {}",
                out.rgb[c],
                neutral[0]
            );
        }
    }

    #[test]
    fn leaving_the_chain_preserves_everything_the_encoder_reads() {
        // The exit boundary is a consuming unwrap so the hand-off moves rather than
        // copies a full-frame buffer. What it must not do is lose anything on the way
        // out, the carried IR plane included: the design says carry the plane rather
        // than consume it, and today's SDR render is the counter-example the new
        // chain declines to copy.
        let ir = vec![0.1, 0.2, 0.3, 0.4];
        let aces = aces_from(2, 2, &[0.25; 12], Some(ir.clone()));
        let expected = bits(&to_p3(aces.rgb()));

        let (linear, _) = render(aces, &params()).unwrap().image.into_parts();

        assert_eq!(linear.width, 2);
        assert_eq!(linear.height, 2);
        assert_eq!(bits(&linear.rgb), expected);
        assert_eq!(linear.ir, Some(ir));
    }

    #[test]
    fn an_ir_free_input_stays_ir_free() {
        // Falsifiability for the test above: the plane must be carried, not minted.
        let (out, _) = render(aces_from(2, 2, &[0.5; 12], None), &params())
            .unwrap()
            .image
            .into_parts();
        assert_eq!(out.ir, None);
    }

    #[test]
    fn the_chain_is_producer_agnostic() {
        // The chain's input is an `AcesCgImage` regardless of which reconstruction
        // produced it — and deliberately *not* only the one a real `--new-flow` run
        // takes (`algo::fixed`): the boundary is the type, so what produces it stays
        // free to change.
        let configs = [
            Reconstruction {
                density: DensityParams::default(),
                curve: DensityCurve::Exponential(ExponentialParams::default()),
            },
            Reconstruction {
                density: DensityParams::default(),
                curve: DensityCurve::Characteristic(CharacteristicParams::default()),
            },
        ];
        for config in configs {
            let base = FilmBase::from([0.5, 0.5, 0.5]);
            let img = LinearImage::new(2, 1, vec![0.1, 0.2, 0.3, 0.4, 0.2, 0.1], None).unwrap();
            let (film, _) =
                reconstruct(&img, &base, &config, crate::types::DmaxInput::default()).unwrap();
            let aces = map_nc_film_rgb_v1(film);
            let expected = bits(&to_p3(aces.rgb()));

            let (out, _) = render(aces, &params()).unwrap().image.into_parts();

            assert_eq!(bits(&out.rgb), expected, "{config:?}");
        }
    }

    #[test]
    fn the_stage_order_is_the_one_the_types_allow() {
        // The chain written out by hand. It compiles only in this order: each
        // stage takes the previous stage's output type, and nothing outside the
        // producing module can mint one — so changing a stage's position is a
        // compile error here. The runtime half — that scene correction's gains land
        // *before* the change of primaries — is
        // `scene_correction_runs_before_the_change_of_primaries`.
        let aces = aces_from(1, 1, &[0.2, 0.4, 0.6], None);
        let p = params();

        let corrected = scene_correction::apply(aces, &p.scene_correction)
            .unwrap()
            .0;
        let graded = look::apply(corrected, &p.look).unwrap();
        let fitted = fit_range::apply(graded, &p.fit_range).unwrap();
        let out: DisplayReferredImage = fit_gamut::apply(fitted, &p.fit_gamut).unwrap();

        assert_eq!(out.into_parts().0.width, 1);
    }

    #[test]
    fn scene_correction_runs_before_the_change_of_primaries() {
        // Per-channel gains do not commute with the 3×3, so the two orders give
        // different pixels — and only one of them is scene correction's contract:
        // white balance acts on ACEScg channels, before the destination's primaries.
        let rgb = [0.2, 0.4, 0.6];
        let aces = aces_from(1, 1, &rgb, None);
        let gains = [2.0f32, 1.0, 0.5];
        let balanced: Vec<f32> = aces
            .rgb()
            .iter()
            .enumerate()
            .map(|(i, v)| v * gains[i % 3])
            .collect();
        let after: Vec<f32> = to_p3(aces.rgb())
            .iter()
            .enumerate()
            .map(|(i, v)| v * gains[i % 3])
            .collect();
        let mut p = params();
        p.scene_correction.white_balance = WhiteBalance::Explicit(gains);

        let rendered = render(aces, &p).unwrap();
        let (out, _) = rendered.image.into_parts();

        assert_eq!(bits(&out.rgb), bits(&to_p3(&balanced)));
        assert_ne!(
            bits(&out.rgb),
            bits(&after),
            "the other order must differ here"
        );
        assert_eq!(rendered.applied[0], ("scene_correction", "white-balance"));
    }

    #[test]
    fn a_boundary_type_can_be_minted_only_by_its_own_stage() {
        // What the order above rests on: each boundary wraps its payload in a field
        // private to the producing module, so no other module can build one. Nothing
        // else pins that — a `pub` added to one of these fields compiles and passes
        // every gate while silently opening the boundary — and a compile-fail harness
        // would cost a dev-dependency for one assertion, so read the sources back
        // instead.
        //
        // A private field is necessary but not sufficient: a second minting route
        // inside the module — a `pub(in crate::pipeline) fn new`, or an
        // `impl From<WorkingBuffer>` — would leave the declaration untouched. So count
        // the construction sites too: the tuple constructor may appear exactly twice —
        // the declaration and `apply` — and never spelled `Self(…)`.
        for (source, name, declaration) in [
            (
                include_str!("scene_correction.rs"),
                "SceneReferredImage",
                "pub struct SceneReferredImage(WorkingBuffer);",
            ),
            (
                include_str!("look.rs"),
                "GradedImage",
                "pub struct GradedImage(WorkingBuffer);",
            ),
            (
                include_str!("fit_range.rs"),
                "RangeFittedImage",
                "pub struct RangeFittedImage(WorkingBuffer);",
            ),
            (
                include_str!("fit_gamut.rs"),
                "DisplayReferredImage",
                "pub struct DisplayReferredImage(WorkingBuffer, DestinationGamut);",
            ),
        ] {
            assert!(
                source.contains(declaration),
                "the payload field must stay private to its stage: `{declaration}`"
            );
            assert_eq!(
                source.matches(&format!("{name}(")).count(),
                2,
                "`{name}` must be minted in one place only: its declaration and `apply`"
            );
            assert!(
                !source.contains("Self("),
                "`{name}`'s module must not mint one through `Self(…)` either"
            );
        }
    }
}
