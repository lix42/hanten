//! **Goldens for the new flow's stages** (`nf-verification/stage-goldens`): the fixed
//! decode, the NC film RGB v1 mapping, and each stage of [`chain`].
//!
//! The new chain's counterpart of `stages::golden`, written fresh rather than
//! re-pointing that module's vectors: `golden::pixels()` is shared with every
//! historical `PIPELINE_FINGERPRINTS` row, and it retires with the legacy path.
//!
//! **Small, curated, decode-independent**: a handful of values fed straight into each
//! stage, no file and no assets, captured as raw `f32` bits. A failure names its stage
//! and sample. Two kinds of vector:
//!
//! - **Per-stage**, so a failure is localized to the stage whose arithmetic moved.
//! - **Threaded** through [`chain::render`], the only vectors that see the chain's
//!   stages *wired together* — which params reach which stage, what the
//!   render reports, and the order the per-channel gains and the 3×3 run in (they do
//!   not commute). They start at the mapped ACEScg, not at the decode: the hand-off
//!   from the decode, and the recipe reaching `DecodeParams`, are the orchestrator's
//!   (`cli::render_new_flow_frame`) and are not covered here.
//!
//! **Which stages are bit-exact and which are windowed is decided by their libm
//! calls.** The decode makes two (`log10`, `powf`) and a fractional exposure one
//! (`exp2`); both are pinned within a window [`reachable_window`] *derives* by
//! enumerating what a 1-ULP-accurate libm can return (CLAUDE.md, determinism). Every
//! other stage here is IEEE `+ − × /` with no FMA contraction — Rust never fuses
//! implicitly — plus sorts and fixed-order sums, so it is pinned bit for bit.
//! A whole-stop exposure still calls `exp2`, but at an integer, and is **taken as
//! exact** there — a power of two every shipped libm returns exactly, and an
//! assumption the 1-ULP premise alone does not give.
//!
//! **Read a failure from the most upstream red golden — and never read a green one as
//! covering the stages above it.** A stage's input can only be minted by the stage
//! before it, so the downstream vectors enter through the mapping, and fit gamut's
//! through scene correction, look and fit range at their identity defaults. A fault in
//! one of those paths can therefore red goldens below it, and the first red one in
//! chain order names the stage that moved. But every downstream vector bypasses the
//! decode (`FilmRgbImage::fixture`), so a decode fault reds only the decode's goldens;
//! and the per-stage vectors below scene correction bypass its multiply (they run it
//! at identity), so a fault there reds its own goldens and the threaded ones only.
//!
//! **NaN bits are never pinned.** A NaN's payload after arithmetic is a property of the
//! target's FPU and libm, not of the chain; an expected NaN is asserted as *a* NaN.
//!
//! Nothing past fit gamut is pinned here: the encode (and any future colour-managed
//! transform) is verified by same-machine before/after, never by a committed vector.
//!
//! A deliberate change to a stage's arithmetic or default recaptures that stage's
//! vector (and the threaded one) in the same change, with a dated note saying why.

use crate::algo::FilmRgbImage;
use crate::algo::fixed::{self, DENSITY_OFFSET, DecodeParams, SCAN_FLOOR};
use crate::pipeline::chain::{self, ChainParams};
use crate::pipeline::fit_gamut::{self, DestinationGamut, FitGamutParams};
use crate::pipeline::fit_range::{self, FitRangeParams};
use crate::pipeline::look::{self, LookParams};
use crate::pipeline::scene_correction::{
    self, SceneCorrection, SceneCorrectionParams, WhiteBalance,
};
use crate::pipeline::working_space::{AcesCgImage, map_nc_film_rgb_v1};
use crate::types::{FilmBase, LinearImage};

// --- shared harness ----------------------------------------------------------

/// An expected value that is "some NaN" — see the module docs on NaN payloads.
const NAN: u32 = 0x7fc0_0000;

/// The accuracy premise every window rests on: a conforming libm is within 1 ULP on
/// `log10`, `powf` and `exp2`. Deliberately the weak bound — `stages::golden`'s
/// `LIBM_MAX_ERROR_ULPS` records why a margin-based argument failed on real targets.
const LIBM_MAX_ERROR_ULPS: i64 = 1;

/// Sanity ceiling on a derived window: a sample landing somewhere the chain amplifies
/// steeply is reported rather than silently granted a huge tolerance.
const MAX_REASONABLE_WINDOW_ULPS: i64 = 200;

fn bits(pixels: &[f32]) -> Vec<u32> {
    pixels.iter().map(|v| v.to_bits()).collect()
}

/// Assert `got` is bit-for-bit the captured `want`, [`NAN`] meaning any NaN.
fn assert_stage_bits(stage: &str, got: &[f32], want: &[u32]) {
    assert_eq!(got.len(), want.len(), "stage `{stage}`: sample count");
    for (i, (&g, &w)) in got.iter().zip(want).enumerate() {
        let wf = f32::from_bits(w);
        if wf.is_nan() {
            assert!(
                g.is_nan(),
                "stage `{stage}` sample {i}: {g:e} where a NaN was captured"
            );
        } else {
            assert_eq!(
                g.to_bits(),
                w,
                "stage `{stage}` sample {i}: {:08x} ({g:e}) drifted from the captured \
                 {w:08x} ({wf:e})",
                g.to_bits()
            );
        }
    }
}

/// A finite `f32` on a line where adjacent values are one apart, `±0` both at zero —
/// so a ULP distance is defined across a sign change as well.
fn ordered(x: f32) -> i64 {
    let magnitude = i64::from(x.to_bits() & 0x7fff_ffff);
    if x.is_sign_negative() {
        -magnitude
    } else {
        magnitude
    }
}

fn ulps_between(a: f32, b: f32) -> i64 {
    debug_assert!(a.is_finite() && b.is_finite(), "{a} / {b}");
    (ordered(a) - ordered(b)).abs()
}

/// Every output a conforming target can produce for one sample, as a window in ULPs
/// around the correctly-rounded one.
///
/// `rounded` is the correctly-rounded value of a libm call's result; a conforming libm
/// may return it or either neighbour, so each is rendered through `render` (the rest
/// of the stage, evaluated target-independently) and the widest excursion taken.
/// `final_call` adds the error of one more libm call at the very end, if the stage
/// makes one.
///
/// **It measures the reachable set's own spread and never looks at the captured
/// value** — a window derived from the value under test widens exactly as far as that
/// value drifts, which `stages::golden::reachable_window` learned by passing a frame
/// moved 115,523 ULPs.
fn reachable_window(render: impl Fn(f32) -> f32, rounded: f32, final_call: i64) -> i64 {
    let centre = render(rounded);
    let widest = [rounded.next_down(), rounded.next_up()]
        .into_iter()
        .map(|neighbour| ulps_between(render(neighbour), centre))
        .max()
        .expect("two neighbours");
    widest + final_call
}

// --- the fixed decode (`algo::fixed`) ----------------------------------------

fn base() -> FilmBase {
    FilmBase::from([0.9, 0.55, 0.42])
}

/// Near-base shadow, mid, dense, the base itself (`D = 0`), above the base (`D < 0`),
/// `0.5 / 0.9`, the three floored cases (zero, negative, subnormal), and the three
/// non-finite ones.
fn decode_scan() -> LinearImage {
    LinearImage::new(
        8,
        1,
        vec![
            0.85,
            0.5,
            0.38,
            0.3,
            0.18,
            0.12,
            0.02,
            0.012,
            0.009,
            0.9,
            0.55,
            0.42,
            0.95,
            0.6,
            0.45,
            0.5,
            0.3,
            0.2,
            0.0,
            -0.25,
            f32::MIN_POSITIVE / 4.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ],
        Some(vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]),
    )
    .unwrap()
}

/// A non-zero offset, distinct per channel. The shipped `DENSITY_OFFSET` is
/// `[0, 0, 0]`, where the `+ offset` term is invisible; this second pass keeps it
/// pinned, and keeps a vector in place if `nf-calibration/offset-question` moves the
/// constant.
const PROBE_OFFSET: [f32; 3] = [-0.05, 0.02, 0.07];

/// The captured decode at the shipped defaults, and at [`PROBE_OFFSET`].
const DECODE_DEFAULT: [u32; 24] = [
    0x3c3e41ab, 0x3c472c7f, 0x3c4467a7, 0x3dbeeaca, 0x3d8a8882, 0x3d841ca2, 0x41a7cc5e, 0x40ccbdff,
    0x403537ec, 0x3c29b443, 0x3c29b443, 0x3c29b443, 0x3c184f87, 0x3c129ff8, 0x3c197151, 0x3d0975d7,
    0x3ceae9de, 0x3cfaab8e, 0x4ffa09f8, 0x4c2dff42, 0x49cd08c6, NAN, NAN, NAN,
];
const DECODE_PROBE_OFFSET: [u32; 24] = [
    0x3c172049, 0x3c5a63c4, 0x3c878e9f, 0x3d97a69f, 0x3d97e60f, 0x3db65d7b, 0x41854976, 0x40e07ecd,
    0x407a26b2, 0x3c06cd00, 0x3c3a13ae, 0x3c6a41c5, 0x3bf1f820, 0x3c20c55e, 0x3c53cf54, 0x3cda6095,
    0x3d00c9ed, 0x3d2d02b3, 0x4fc69ce2, 0x4c3ec8b5, 0x4a0d835e, NAN, NAN, NAN,
];

/// The anchor the defaults resolve (`MID_ABOVE_BASE + 0.745 / CONTRAST`).
const DECODE_ANCHOR: u32 = 0x3f7e0b8d;

fn decode_params(offset: [f32; 3]) -> DecodeParams {
    DecodeParams {
        offset,
        ..DecodeParams::default()
    }
}

/// The correctly-rounded density of each finite sample, `None` for a non-finite one.
/// Written as the decode writes it: the floor and the division in f32, then `log10`.
fn correctly_rounded_densities() -> Vec<Option<f32>> {
    let base = <[f32; 3]>::from(base());
    decode_scan()
        .rgb
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            s.is_finite().then(|| {
                let ratio = s.max(SCAN_FLOOR) / base[i % 3];
                -(f64::from(ratio).log10()) as f32
            })
        })
        .collect()
}

/// The decode after its `log10`, correctly rounded: the calibration and exponent in
/// f32 as the decode writes them, the `10^` in f64.
fn decode_from_density(d: f32, channel: usize, params: &DecodeParams) -> f32 {
    let anchor = params.anchor.anchor(params.contrast);
    let corrected = params.scale[channel] * d + params.offset[channel];
    10f64.powf(f64::from(params.contrast * (corrected - anchor))) as f32
}

#[test]
fn golden_decode_is_correct_within_its_libm_window() {
    let densities = correctly_rounded_densities();
    for (offset, expected) in [
        (DENSITY_OFFSET, &DECODE_DEFAULT),
        (PROBE_OFFSET, &DECODE_PROBE_OFFSET),
    ] {
        let params = decode_params(offset);
        let (film, report) = fixed::decode(&decode_scan(), &base(), &params).unwrap();
        assert_eq!(film.rgb().len(), expected.len());

        for (i, (&got, &want)) in film.rgb().iter().zip(expected).enumerate() {
            let Some(d) = densities[i] else {
                assert!(got.is_nan(), "stage `decode` sample {i}: {got:e}, not NaN");
                continue;
            };
            let captured = f32::from_bits(want);
            let window = reachable_window(
                |d| decode_from_density(d, i % 3, &params),
                d,
                LIBM_MAX_ERROR_ULPS,
            );
            let drift = ulps_between(got, captured);
            assert!(
                drift <= window,
                "stage `decode` (offset {offset:?}) sample {i}: {:08x} is {drift} ULP from \
                 the captured {want:08x}, outside the {window} ULP a conforming libm can reach",
                got.to_bits()
            );
        }

        // IEEE arithmetic on constants: bit-exact on every target.
        assert_eq!(
            report.anchor.to_bits(),
            DECODE_ANCHOR,
            "stage `decode`: anchor"
        );
        assert_eq!(report.anchor_rule, "mid-at-base-offset");
        assert!(!report.reads_reference);
        assert_eq!(
            film.ir(),
            Some(&[0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8][..])
        );
    }
}

/// The capture is what a correctly-rounded chain produces, the host's `log10` and `powf`
/// are conforming, and
/// no window is suspiciously wide — the three things the golden above assumes.
#[test]
fn the_decode_capture_is_correctly_rounded_and_the_host_conforms() {
    let densities = correctly_rounded_densities();
    let scan = decode_scan();
    let base = <[f32; 3]>::from(base());
    let mut widest = 0;
    for (offset, expected) in [
        (DENSITY_OFFSET, &DECODE_DEFAULT),
        (PROBE_OFFSET, &DECODE_PROBE_OFFSET),
    ] {
        let params = decode_params(offset);
        for (i, &want) in expected.iter().enumerate() {
            let Some(d) = densities[i] else {
                assert_eq!(want, NAN, "sample {i}: a non-finite scan decodes to NaN");
                continue;
            };
            // Conformance, never correct rounding: that is exactly what varies.
            let host = -(scan.rgb[i].max(SCAN_FLOOR) / base[i % 3]).log10();
            let off = ulps_between(host, d);
            assert!(
                off <= LIBM_MAX_ERROR_ULPS,
                "sample {i}: this host's `log10` is {off} ULP from the correctly rounded {d:e}"
            );
            // The second libm call, `powf`, at the exponent the decode forms from `d`.
            let corrected = params.scale[i % 3] * d + params.offset[i % 3];
            let exponent = params.contrast * (corrected - params.anchor.anchor(params.contrast));
            let off = ulps_between(10f32.powf(exponent), decode_from_density(d, i % 3, &params));
            assert!(
                off <= LIBM_MAX_ERROR_ULPS,
                "sample {i}: this host's `powf` is {off} ULP from the correctly rounded 10^{exponent}"
            );
            assert_eq!(
                decode_from_density(d, i % 3, &params).to_bits(),
                want,
                "sample {i} (offset {offset:?}): the capture is not the correctly-rounded decode"
            );
            widest = widest.max(reachable_window(
                |d| decode_from_density(d, i % 3, &params),
                d,
                LIBM_MAX_ERROR_ULPS,
            ));
        }
    }
    assert!(
        widest <= MAX_REASONABLE_WINDOW_ULPS,
        "the widest derived decode window is {widest} ULP — understand it before accepting it"
    );
}

// --- the working-space mapping and the chain ---------------------------------

/// Film-RGB values for everything downstream of the decode: mid-grey, a saturated
/// blue, a red with a negative channel (outside the film cube, which an unclamped
/// reconstruction can produce, and outside Display P3 after fit gamut), diffuse white,
/// above white, black, a near-black, and a pixel with a NaN channel. With an IR plane, which every stage
/// must carry.
fn film() -> FilmRgbImage {
    FilmRgbImage::fixture(
        LinearImage::new(
            8,
            1,
            vec![
                0.18,
                0.18,
                0.18,
                0.002,
                0.004,
                0.9,
                0.9,
                0.004,
                -0.02,
                1.0,
                1.0,
                1.0,
                4.0,
                3.0,
                2.5,
                0.0,
                0.0,
                0.0,
                0.003,
                0.004,
                0.002,
                0.4,
                f32::NAN,
                0.2,
            ],
            Some(FILM_IR.to_vec()),
        )
        .unwrap(),
    )
}

const FILM_IR: [f32; 8] = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];

fn aces() -> AcesCgImage {
    map_nc_film_rgb_v1(film())
}

const MAPPED: [u32; 24] = [
    0x3e3851ec, 0x3e3851ed, 0x3e3851eb, 0x3d393ec1, 0x3c825bc6, 0x3f48872e, 0x3f0d5cdc, 0x3d88563e,
    0x3ad1310b, 0x3f800000, 0x3f800000, 0x3f7fffff, 0x4065b8db, 0x40440fdb, 0x40257c3e, 0x00000000,
    0x00000000, 0x00000000, 0x3b57c101, 0x3b7fc7d4, 0x3b12c8db, NAN, NAN, NAN,
];

#[test]
fn golden_working_space_mapping_is_bit_identical() {
    // The frozen `nc-film-rgb-v1` identifier's runtime half: its matrix is audited in
    // `colorimetry`, but only this pins what the mapper does with it.
    let out = aces();
    assert_stage_bits("working-space", out.rgb(), &MAPPED);
    assert_eq!(out.ir(), Some(&FILM_IR[..]));
}

fn scene_correct(params: &SceneCorrectionParams) -> Vec<f32> {
    let (out, _) = scene_correction::apply(aces(), params).unwrap();
    out.into_buffer().into_linear().rgb
}

const SCENE_STATED: [u32; 24] = [
    0x3ee66667, 0x3eb851ed, 0x3e3851eb, 0x3de78e71, 0x3d025bc6, 0x3f48872e, 0x3fb0b413, 0x3e08563e,
    0x3ad1310b, 0x40200000, 0x40000000, 0x3f7fffff, 0x410f9389, 0x40c40fdb, 0x40257c3e, 0x00000000,
    0x00000000, 0x00000000, 0x3c06d8a1, 0x3bffc7d4, 0x3b12c8db, NAN, NAN, NAN,
];

#[test]
fn golden_scene_correction_stated_is_bit_identical() {
    // A whole-stop exposure: `2^1` is exact on every conforming `exp2`, so this is
    // pure IEEE arithmetic.
    let params = SceneCorrectionParams {
        white_balance: WhiteBalance::Explicit([1.25, 1.0, 0.5]),
        exposure: 1.0,
    };
    assert_stage_bits("scene-correction", &scene_correct(&params), &SCENE_STATED);
}

const SCENE_FRACTIONAL: [u32; 24] = [
    0x3e8f5e09, 0x3e82557d, 0x3e6a99dd, 0x3d90163f, 0x3cb85ad0, 0x3f7f3b03, 0x3f5be8a8, 0x3dc0cf39,
    0x3b0520f2, 0x3fc71f0c, 0x3fb504f3, 0x3fa2ead9, 0x40b2ae8e, 0x408aa300, 0x4052a0df, 0x00000000,
    0x00000000, 0x00000000, 0x3ba7d132, 0x3bb4dd3b, 0x3b3ad386, NAN, NAN, NAN,
];
const FRACTIONAL_EV: f32 = 0.5;
const FRACTIONAL_WB: [f32; 3] = [1.1, 1.0, 0.9];

#[test]
fn golden_scene_correction_fractional_exposure_is_correct_within_its_libm_window() {
    // `2^0.5` goes through `exp2`, which may differ by a ULP across targets; everything
    // after it is IEEE. The window enumerates the three gains a conforming libm returns.
    let rounded = (f64::from(FRACTIONAL_EV).exp2()) as f32;
    let host = FRACTIONAL_EV.exp2();
    assert!(
        ulps_between(host, rounded) <= LIBM_MAX_ERROR_ULPS,
        "this host's `exp2` is not conforming"
    );
    let params = SceneCorrectionParams {
        white_balance: WhiteBalance::Explicit(FRACTIONAL_WB),
        exposure: FRACTIONAL_EV,
    };
    let input = aces().rgb().to_vec();
    let got = scene_correct(&params);
    assert_eq!(got.len(), SCENE_FRACTIONAL.len());
    let mut widest = 0;
    for (i, (&g, &want)) in got.iter().zip(&SCENE_FRACTIONAL).enumerate() {
        if want == NAN {
            assert!(g.is_nan(), "stage `scene-correction` sample {i}: {g:e}");
            continue;
        }
        let render = |gain: f32| input[i] * (FRACTIONAL_WB[i % 3] * gain);
        assert_eq!(
            render(rounded).to_bits(),
            want,
            "sample {i}: capture integrity"
        );
        let window = reachable_window(render, rounded, 0);
        widest = widest.max(window);
        let drift = ulps_between(g, f32::from_bits(want));
        assert!(
            drift <= window,
            "stage `scene-correction` sample {i}: {drift} ULP from the capture, outside the \
             {window} ULP a conforming `exp2` can reach"
        );
    }
    assert!(widest <= MAX_REASONABLE_WINDOW_ULPS, "{widest}");
}

/// [`aces`] through scene correction, look and fit range at their defaults — the only
/// way to mint fit gamut's input, and, today, three identities.
fn through_fit_range() -> fit_range::RangeFittedImage {
    let (corrected, _) =
        scene_correction::apply(aces(), &SceneCorrectionParams::default()).unwrap();
    let graded = look::apply(corrected, &LookParams::default()).unwrap();
    fit_range::apply(graded, &FitRangeParams::default()).unwrap()
}

#[test]
fn golden_look_and_fit_range_are_bit_exact_identities() {
    // Identity is what these two stages *are* today, so their golden is the input.
    // The first look control under `nf-look` and `nf-display-stages/fit-range` replace
    // this with captured vectors of their own when they give the stages arithmetic.
    let input = bits(aces().rgb());
    let fitted = through_fit_range();
    let out = fitted.into_buffer().into_linear();
    assert_eq!(
        bits(&out.rgb),
        input,
        "stage `look` or `fit-range` moved a pixel"
    );
    assert_eq!(out.ir.as_deref(), Some(&FILM_IR[..]));
}

const FIT_GAMUT_P3: [u32; 24] = [
    0x3e3851ed, 0x3e3851ed, 0x3e3851ec, 0x3b1a5990, 0x3b80e4f0, 0x3f51dddf, 0x3f3dad52, 0x3d0a3515,
    0xbb26e27a, 0x3f800001, 0x3f800000, 0x3f7fffff, 0x4074a339, 0x40421fdb, 0x4023f4e7, 0x00000000,
    0x00000000, 0x00000000, 0x3b503e3d, 0x3b81fbfb, 0x3b0dae49, NAN, NAN, NAN,
];

#[test]
fn golden_fit_gamut_is_bit_identical() {
    let fitted = through_fit_range();
    let params = FitGamutParams {
        target: DestinationGamut::DisplayP3,
    };
    let (out, gamut) = fit_gamut::apply(fitted, &params).unwrap().into_parts();
    assert_stage_bits("fit-gamut", &out.rgb, &FIT_GAMUT_P3);
    assert_eq!(gamut, DestinationGamut::DisplayP3);
    assert_eq!(out.ir.as_deref(), Some(&FILM_IR[..]));
}

// --- threaded ----------------------------------------------------------------

const THREADED: [u32; 24] = [
    0x3dfe5b90, 0x3db651d4, 0x3d2f5812, 0x3cba6090, 0x3b86c73e, 0x3e51a4de, 0x3eee6f0c, 0x3c4616a9,
    0xbaf134b8, 0x3f30a324, 0x3efd38c1, 0x3e738889, 0x4024d571, 0x3fbf370b, 0x3f1a4cc3, 0x00000000,
    0x00000000, 0x00000000, 0x3b0fe646, 0x3b00970d, 0x3a015abe, NAN, NAN, NAN,
];

#[test]
fn golden_the_chain_threaded_is_bit_identical() {
    // Non-identity scene correction on purpose: per-channel gains and the destination
    // matrix do not commute, so a chain that ran them in the other order, or handed a
    // stage the wrong params, lands elsewhere even though every per-stage golden passes.
    let params = ChainParams {
        scene_correction: SceneCorrectionParams {
            white_balance: WhiteBalance::Explicit([1.25, 1.0, 0.5]),
            exposure: -1.0,
        },
        look: LookParams::default(),
        fit_range: FitRangeParams::default(),
        fit_gamut: FitGamutParams {
            target: DestinationGamut::DisplayP3,
        },
    };
    let rendered = chain::render(aces(), &params).unwrap();
    assert_eq!(
        rendered.applied,
        [
            ("scene_correction", "white-balance+exposure"),
            ("look", "identity"),
            ("fit_range", "identity"),
            ("fit_gamut", "acescg-to-display-p3-matrix"),
        ],
        "chain (threaded): the stage list the render reports"
    );
    assert_eq!(
        rendered.scene_correction,
        SceneCorrection {
            white_balance: [1.25, 1.0, 0.5],
            exposure: -1.0,
        },
        "chain (threaded): the scene correction the render reports"
    );
    let (out, gamut) = rendered.image.into_parts();
    assert_stage_bits("chain (threaded)", &out.rgb, &THREADED);
    assert_eq!(gamut, DestinationGamut::DisplayP3);
    assert_eq!(out.ir.as_deref(), Some(&FILM_IR[..]));
}
