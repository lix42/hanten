//! The new rendering chain, composed: scene correction → look → fit range → fit
//! gamut.
//!
//! The chain `--new-flow` selects (`docs/design-update.md` Part 2,
//! `docs/nf-migration.md`). Every stage is an **identity pass** today —
//! `nf-core/stage-skeleton` builds the boundaries; the stage epics fill them —
//! and it is deliberately not reachable from the CLI yet: the seam in
//! `cli::convert_frame` still refuses, because there is no destination behind it.
//! The decode ahead of it exists (`algo::fixed`,
//! `nf-reconstruction/fixed-decode`); `nf-core/minimal-end-to-end` wires that to
//! this chain and this chain to an output.
//!
//! **The order is carried by the types, not by this function.** Each stage's
//! input is the previous stage's output type, and each of those can be minted
//! only inside the module that produces it — so a chain composed out of order
//! does not compile. That is the whole point of the skeleton: the boundaries are
//! decided once, deliberately, rather than falling out of whichever stage ships
//! first.
//!
//! Entering the chain, crossing each boundary, and **leaving** it
//! (`DisplayReferredImage::into_linear`) all move the pixel buffers, so a type per
//! stage costs no allocation. The exit is the one worth stating separately: the
//! encoder takes `&LinearImage`, so a boundary with only `&`-accessors would force
//! a full-frame copy at the hand-off — ~0.9 GB on a 74.6 MP scan — and the "no
//! allocation" claim would be false at exactly the point it matters most. What each
//! stage then *does* with the buffer it owns is `nf-core/buffer-strategy`'s to
//! settle.

use crate::pipeline::fit_gamut::{self, DisplayReferredImage, FitGamutParams};
use crate::pipeline::fit_range::{self, FitRangeParams};
use crate::pipeline::look::{self, LookParams};
use crate::pipeline::scene_correction::{self, SceneCorrectionParams};
use crate::pipeline::working_space::AcesCgImage;
use crate::types::Result;

/// Every stage's parameters, in chain order.
///
/// Not a recipe type: `nf-core/recipe-schema` owns which sections exist and how a
/// recipe declares them, and it depends on this task. These structs exist so a
/// stage's signature is settled now and does not change when its knobs arrive.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChainParams {
    pub scene_correction: SceneCorrectionParams,
    pub look: LookParams,
    pub fit_range: FitRangeParams,
    pub fit_gamut: FitGamutParams,
}

/// Render an [`AcesCgImage`] through the new chain.
///
/// **While every stage is an identity** this is one too, bit-exactly — including
/// non-finite samples and values outside `[0, 1]`, which ride through untouched:
/// the working range is preserved to the encoder, which is the only place clamping
/// happens. The clamping boundary is permanent; the *untouched* half is a statement
/// about today's identity stages, not a contract — a filled fit range may
/// legitimately refuse a non-finite sample, which is what the `Result` below is for.
///
/// **Fallible by construction.** Every stage's signature returns a `Result` and so
/// does this, although none of them can fail yet. That is the point of settling the
/// boundaries once: every stage this chain will host has a *fallible* counterpart in
/// the shipped code — `render_split::display_source`, `sdr::render` (which errors on
/// a non-finite sample) and `hdr::render_linear` all return `Result` — so a stage
/// that gains its arithmetic would otherwise change its signature, this function's,
/// every call site and every test here. The cost while the stages are identities is
/// an `Ok` wrapper.
///
/// The single-branch shape is deliberate. The SDR/HDR split belongs below the
/// look — a gain map requires the two renditions to agree below diffuse white —
/// but where exactly, and whether a single-rendition destination goes through the
/// branch point at all, is `nf-display-stages/branch-contract`'s open question.
/// It splits *from* [`GradedImage`], whichever way it lands.
///
/// [`GradedImage`]: crate::pipeline::look::GradedImage
#[allow(dead_code)] // wired to a destination by `nf-core/minimal-end-to-end`.
pub fn render(image: AcesCgImage, params: &ChainParams) -> Result<DisplayReferredImage> {
    let corrected = scene_correction::apply(image, &params.scene_correction)?;
    let graded = look::apply(corrected, &params.look)?;
    let fitted = fit_range::apply(graded, &params.fit_range)?;
    fit_gamut::apply(fitted, &params.fit_gamut)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::reconstruct;
    use crate::pipeline::working_space::map_nc_film_rgb_v1;
    use crate::types::{
        DensityCurve, DensityParams, ExponentialParams, FilmBase, LinearImage, Reconstruction,
        SigmoidParams,
    };

    /// Build an `AcesCgImage` whose *film RGB* input was exactly `rgb`, through
    /// the real `reconstruct → map_nc_film_rgb_v1` path — the only way to mint
    /// one. `simple` with a unit base and a pre-inverted scan gives
    /// `1 − (1 − target)/1 == target` bit-for-bit, which is what lets a test
    /// place a chosen value — including a non-finite one — into the chain's
    /// input. (`simple` is refused by `--new-flow` at the CLI; that is an
    /// availability rule about what a *user* may ask for, and says nothing about
    /// which producer a stage test may use. `chain_is_producer_agnostic` covers
    /// the curves a real run would take.)
    fn aces_from(width: u32, height: u32, rgb: &[f32], ir: Option<Vec<f32>>) -> AcesCgImage {
        let base = FilmBase::from([1.0, 1.0, 1.0]);
        let scan: Vec<f32> = rgb.iter().map(|&t| 1.0 - t).collect();
        let img = LinearImage::new(width, height, scan, ir).unwrap();
        let (film, _) = reconstruct(&img, &base, &Reconstruction::Simple, None).unwrap();
        map_nc_film_rgb_v1(film)
    }

    /// Bit patterns, not values: `NaN != NaN`, so an `==` comparison would pass
    /// over exactly the samples this asserts survive untouched.
    fn bits(pixels: &[f32]) -> Vec<u32> {
        pixels.iter().map(|v| v.to_bits()).collect()
    }

    #[test]
    fn the_chain_is_a_bit_exact_identity() {
        // Ordinary values, both ends of the working range, and the awkward ones:
        // above 1.0 (unclamped is the contract), below 0.0 (a wide-gamut linear
        // space contains them), and non-finite (passed through for the encoder to
        // count, never silently repaired).
        let rgb = [
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
        let aces = aces_from(3, 1, &rgb, None);
        let before = bits(aces.rgb());

        let out = render(aces, &ChainParams::default()).unwrap();

        assert_eq!(
            bits(&out.into_linear().rgb),
            before,
            "a stage moved a pixel"
        );
    }

    #[test]
    fn a_one_ulp_move_in_the_rendered_output_is_visible() {
        // Falsifiability control for the identity test above, which is evidence only
        // if its comparison can fail on the values this chain actually carries. So:
        // render, then move one sample of the *output* by a single ULP and confirm
        // the same comparison reds.
        //
        // The comparison is against the **unperturbed output**, not against the
        // input: asserting against the input would also pass on any chain that stops
        // being an identity, i.e. exactly when this control stops exercising ULP
        // detection at all.
        //
        // What this does **not** do is make a stage non-identity — that needs a
        // deliberately broken build, done by hand when the chain landed (recorded in
        // `docs/progress/nf-core.md`, where a one-ULP move inside `look` red the
        // identity tests). This pins the detector, not the stages.
        let aces = aces_from(
            3,
            1,
            &[0.0, 0.18, 1.0, 5.0, -0.25, 0.5, 0.25, 0.75, 0.9],
            None,
        );

        let rendered = bits(
            &render(aces, &ChainParams::default())
                .unwrap()
                .into_linear()
                .rgb,
        );
        let mut moved = rendered.clone();

        // No NaN in this vector, so `next_up` genuinely steps — on a NaN it would
        // return a NaN again and the assertion below could pass for the wrong reason.
        let sample = f32::from_bits(moved[4]);
        assert!(sample.is_finite(), "the perturbed sample must be finite");
        moved[4] = sample.next_up().to_bits();

        assert_ne!(
            moved, rendered,
            "a one-ULP move must be visible to the compare"
        );
    }

    #[test]
    fn leaving_the_chain_preserves_everything_the_encoder_reads() {
        // The exit boundary, which `nf-core/minimal-end-to-end` hands to
        // `io::encode` — a consuming unwrap so the hand-off moves rather than
        // copies a full-frame buffer. What it must not do is lose anything on the
        // way out, the carried IR plane included: the design says carry the plane
        // rather than consume it, and today's SDR render is the counter-example the
        // new chain declines to copy.
        let ir = vec![0.1, 0.2, 0.3, 0.4];
        let aces = aces_from(2, 2, &[0.25; 12], Some(ir.clone()));
        let before = bits(aces.rgb());

        let linear = render(aces, &ChainParams::default()).unwrap().into_linear();

        assert_eq!(linear.width, 2);
        assert_eq!(linear.height, 2);
        assert_eq!(bits(&linear.rgb), before);
        assert_eq!(linear.ir, Some(ir));
    }

    #[test]
    fn an_ir_free_input_stays_ir_free() {
        // Falsifiability for the test above: the plane must be carried, not minted.
        let out = render(aces_from(2, 2, &[0.5; 12], None), &ChainParams::default()).unwrap();
        assert_eq!(out.into_linear().ir, None);
    }

    #[test]
    fn the_chain_is_producer_agnostic() {
        // The chain's input is an `AcesCgImage` regardless of which reconstruction
        // produced it — and deliberately *not* only the one a real `--new-flow` run
        // takes: that is `algo::fixed::DecodeParams`, and a recipe stating
        // `reconstruction` is refused outright. These legacy configurations are here
        // because the boundary is the type, so what produces it stays free to change.
        let configs = [
            Reconstruction::Simple,
            Reconstruction::Density {
                density: DensityParams::default(),
                curve: DensityCurve::Exponential(ExponentialParams::default()),
            },
            Reconstruction::Density {
                density: DensityParams::default(),
                curve: DensityCurve::Sigmoid(SigmoidParams::default()),
            },
        ];
        for config in configs {
            let base = FilmBase::from([0.5, 0.5, 0.5]);
            let img = LinearImage::new(2, 1, vec![0.1, 0.2, 0.3, 0.4, 0.2, 0.1], None).unwrap();
            let (film, _) = reconstruct(&img, &base, &config, None).unwrap();
            let aces = map_nc_film_rgb_v1(film);
            let before = bits(aces.rgb());

            let out = render(aces, &ChainParams::default()).unwrap();

            assert_eq!(
                bits(&out.into_linear().rgb),
                before,
                "{config:?} moved a pixel"
            );
        }
    }

    #[test]
    fn the_stage_order_is_the_one_the_types_allow() {
        // The chain written out by hand. It compiles only in this order: each
        // stage takes the previous stage's output type, and nothing outside the
        // producing module can mint one — so changing a stage's position is a
        // compile error here.
        //
        // Note what this does *not* claim. While every stage is an identity,
        // reordering `render` alone is unobservable at runtime — any order
        // produces the same bytes — so no runtime assertion can catch it, and one
        // written as if it could would be vacuous. Real order coverage arrives
        // with the first stage that does something.
        let aces = aces_from(1, 1, &[0.2, 0.4, 0.6], None);
        let params = ChainParams::default();

        let corrected = scene_correction::apply(aces, &params.scene_correction).unwrap();
        let graded = look::apply(corrected, &params.look).unwrap();
        let fitted = fit_range::apply(graded, &params.fit_range).unwrap();
        let out: DisplayReferredImage = fit_gamut::apply(fitted, &params.fit_gamut).unwrap();

        assert_eq!(out.into_linear().width, 1);
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
        // inside the module — `pub(in crate::pipeline) fn new(buf: WorkingBuffer) ->
        // Self`, or an `impl From<WorkingBuffer>` — would leave the declaration
        // untouched, and any `pipeline` module can already obtain a `WorkingBuffer`
        // (`WorkingBuffer::from_aces` is `pub(in crate::pipeline)`). So count the
        // construction sites too: the tuple constructor may appear exactly twice —
        // the declaration and `apply` — and never spelled `Self(…)`.
        for (source, name) in [
            (include_str!("scene_correction.rs"), "SceneReferredImage"),
            (include_str!("look.rs"), "GradedImage"),
            (include_str!("fit_range.rs"), "RangeFittedImage"),
            (include_str!("fit_gamut.rs"), "DisplayReferredImage"),
        ] {
            let declaration = format!("pub struct {name}(WorkingBuffer);");
            assert!(
                source.contains(&declaration),
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
