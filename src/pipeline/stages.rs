//! Stage wiring as pure functions — threads film-base → reconstruction → the
//! selected output branch together for the orchestrator to call.
//!
//! This is the in-memory core of the `convert` pipeline (design-spec §6, stages
//! 3–5b). [`render`] owns the legacy/film-master branches;
//! [`render_display_source`] is the CLI-reachable shared display source for the
//! explicit `ultra-hdr-v1` path, whose SDR/HDR/gain-map rendering and packaging
//! the CLI orchestrates afterward. Film-base estimation (stage 2) is the
//! orchestrator's, and
//! decode (stage 1) and encode (the final stage) are I/O and stay with
//! the orchestrator (`cli`); everything here is pure `(input, params) -> output`
//! so it composes and unit-tests without touching the filesystem — with one
//! documented exception: [`render`] reads a wall clock to fill [`StageTimings`]
//! for the telemetry record (a report-only channel; the pixels stay
//! deterministic and untouched by the measurement).
//!
//! The resolved [`OutputPreset`] routes through these stage entrypoints:
//!
//! - **`legacy`** (no preset, the default) — the frozen transitional path
//!   `reconstruct → finish_print → color::to_output`: the print controls run
//!   *before* the working→output ICC transform, exactly as they did before
//!   presets existed. `golden` (below, `#[cfg(test)]`) pins its
//!   **pre-colour-transform** pixels bit-for-bit by calling
//!   [`reconstruct_and_print`] directly, and
//!   `legacy_preset_render_is_the_frozen_reconstruct_print_colour_sequence` pins
//!   that this branch of [`render`] is still exactly that sequence — the boundary
//!   `golden` cannot see, because it never crosses the preset `match`.
//! - **`film-master`** — `reconstruct → map_nc_film_rgb_v1 → render_split::film_master`:
//!   the mapped unclamped linear ACEScg buffer is encoded directly with the
//!   ACEScg ICC attached and **no** colour transform, print control, or display
//!   rendering. Running `color::to_output` here would re-apply the
//!   Rec.709→ACEScg matrix on values that already crossed it, so the master
//!   deliberately bypasses that stage and only fetches the profile blob.
//! - **`ultra-hdr-v1`** — `render_display_source` reconstructs once, maps to
//!   ACEScg, and applies the shared print controls once; the orchestrator then
//!   feeds that source to both display renderers and the legacy gain-map encoder.

use std::time::Instant;

use crate::algo;
use crate::pipeline::color::{self, OutputSpace};
use crate::pipeline::display_tone::DisplayTone;
use crate::pipeline::{render_split, sdr, working_space};
use crate::types::{
    FilmBase, LinearImage, OutputParams, OutputPreset, PrintParams, Reconstruction, Result,
};

/// The in-memory pipeline result the orchestrator hands to the encoder: the
/// output-color-transformed positive image and the ICC blob to embed alongside
/// it.
pub struct Rendered {
    pub image: LinearImage,
    pub icc: Vec<u8>,
    /// Resolved-value diagnostics (e.g. the `Dmax` anchor the curve used) for
    /// the JSON report.
    pub convert: ConvertReport,
    /// Wall-clock per-stage timings measured around the calls in [`render`], for
    /// the telemetry record's `timing_ms` block. Like [`ConvertReport`], a
    /// report-only channel: it is never serialized into the recipe sidecar and
    /// never read back by any stage, so the byte-identical-output determinism
    /// contract is untouched.
    pub timings: StageTimings,
}

/// The shared display source and diagnostics used to build a gain-map output.
pub struct DisplaySource {
    pub shared: render_split::SharedDisplaySource,
    pub convert: ConvertReport,
    pub timings: StageTimings,
}

/// Per-conversion diagnostics the render surfaces for the JSON report — the
/// reconstruction stage's resolved values plus the legacy print stage's
/// resolved gains. A reporting channel, not a control surface (controls live in
/// the recipe structs).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConvertReport {
    /// The resolved **reference** density (`curve.dmax`) — the roll calibration, and the
    /// value to freeze back into a recipe. `None` for `simple` (no curve stage) and for
    /// the exponential curve with `dmax = none`.
    ///
    /// Not necessarily the density that rendered to `1.0`: the sigmoid derives its anchor
    /// from this reference via `AnchorPlacement`. See [`Self::curve_anchor`].
    pub dmax: Option<f32>,
    /// The **derived** anchor the curve used — the corrected density that rendered to
    /// `1.0`, hence the black floor at `10^(−contrast·curve_anchor)`. Equal to
    /// [`Self::dmax`] for the exponential curve and the sigmoid's `white-at-dmax`
    /// placement; larger under the default mid-grey placement.
    pub curve_anchor: Option<f32>,
    /// The resolved stage-4 white-balance gains `[r, g, b]` the legacy print
    /// render applied — the explicit gains, or the auto-estimated ones
    /// (`print.white_balance = gray-world | percentile`). Reported so a roll can
    /// freeze one frame's estimate into a recipe / `--white-balance` (measure
    /// once, reuse). `None` for `simple` (no print stage).
    pub white_balance: Option<[f32; 3]>,
    /// How far the frame fell outside the stock's published curve, per channel — `Some`
    /// only for the characteristic curve. Carried out so the orchestrator can warn: an
    /// out-of-table sample is extrapolated, not measured, and a frame with many of them is
    /// being rendered off the published data.
    pub out_of_table: Option<crate::algo::film_stock::OutOfTable>,
    /// The resolved regional-balance tone-ramp range `[lo, hi]` (corrected
    /// density), when the density reconstruction applied a shadow/highlight
    /// balance. `None` for `simple` or when both balances are the neutral
    /// `[0, 0, 0]`. Reported so a roll can reuse one frame's measured range via
    /// `--balance-range` (design-spec §9).
    pub balance_range: Option<[f32; 2]>,
}

/// Stages 3–4 in memory: reconstruct the negative into the typed film positive
/// ([`algo::reconstruct`], stage 3 — every path returns a
/// [`FilmRgbImage`](algo::FilmRgbImage)), then run the legacy no-preset print
/// finishing ([`algo::finish_print`], stage 4) back to the plain working-space
/// image the output color transform consumes. Split out from [`render`] so the
/// pre-color-transform pixels are directly testable — the golden fixtures below
/// pin them bit-for-bit against the pre-split converters.
pub(crate) fn reconstruct_and_print(
    image: &LinearImage,
    film_base: &FilmBase,
    reconstruction: &Reconstruction,
    print: &PrintParams,
) -> Result<(LinearImage, ConvertReport)> {
    let (film, recon) = algo::reconstruct(image, film_base, reconstruction)?;
    let (positive, white_balance) = algo::finish_print(film, reconstruction, print)?;
    Ok((
        positive,
        ConvertReport {
            dmax: recon.dmax,
            curve_anchor: recon.curve_anchor,
            white_balance,
            balance_range: recon.balance_range,
            out_of_table: recon.out_of_table,
        },
    ))
}

/// Run the in-memory pipeline on a decoded image and an **already-resolved** film
/// base, taking whichever branch `output_params.preset` selects: `legacy`
/// (reconstruct → print render → working→output ICC transform) or `film-master`
/// (reconstruct → NC film RGB v1 → the unwrapped ACEScg buffer, no transform).
/// Returns the image to encode and the ICC blob to embed alongside it.
///
/// Film-base estimation (stage 2) is deliberately **not** done here — the
/// orchestrator resolves the base first (via [`film_base::estimate`]) so it can
/// surface the estimate's quality warnings before this fallible render runs (a
/// downstream failure must not swallow the "non-uniform region" warning that
/// explains a bad base). Total in its inputs: any failure (a degenerate film
/// base, an unusable curve anchor, an unreadable custom ICC profile) surfaces as
/// an [`NcError`](crate::types::NcError) with the right exit code, never a
/// silently-wrong image. The IR plane is carried through untouched (Step-1 rule:
/// preserve, don't consume).
///
/// The reconstruction+print and output-color stages are each timed with
/// [`Instant`] pairs (returned as [`StageTimings`] for the telemetry record; the
/// film-base stage is timed by the orchestrator, which owns that estimation).
/// The measurement is the one deliberate impurity here; it never reads back into
/// the pipeline, so the same inputs still produce bit-identical pixels and ICC
/// whether or not telemetry is collected.
pub fn render(
    image: &LinearImage,
    film_base: &FilmBase,
    reconstruction: &Reconstruction,
    print: &PrintParams,
    output_params: &OutputParams,
) -> Result<Rendered> {
    match output_params.preset {
        // `custom` is the same legacy branch, explicitly chosen rather than
        // inherited from the default — same bytes, different provenance.
        OutputPreset::Legacy | OutputPreset::Custom => {
            render_legacy(image, film_base, reconstruction, print, output_params)
        }
        OutputPreset::FilmMaster => render_film_master(image, film_base, reconstruction),
        // Every display preset renders from the shared source instead, so that
        // reconstruction and the print controls resolve exactly once for whichever
        // rendition(s) the preset needs.
        OutputPreset::UltraHdrV1
        | OutputPreset::GainMapHdr
        | OutputPreset::HdrPq
        | OutputPreset::HdrHlg
        | OutputPreset::HdrLinearTiff
        | OutputPreset::HdrPqTiff
        | OutputPreset::HdrHlgTiff
        | OutputPreset::DisplayP3
        | OutputPreset::Compatibility => Err(crate::types::NcError::Other(format!(
            "`{}` must use stages::render_display_source",
            output_params.preset.name()
        ))),
    }
}

/// The `display-p3` / `compatibility` render: the shared display source, one SDR
/// rendition in the requested gamut, and the output-encoded pixels plus their ICC.
///
/// Returns the same [`Rendered`] the legacy branch does, which is deliberate — an
/// SDR preset's product *is* a rendered image and a profile, so it reuses the
/// existing 16-bit TIFF encode path rather than introducing a container of its own.
/// What differs from `legacy` is everything upstream: this crosses the ACEScg
/// boundary and runs the shared print controls plus `pipeline::sdr`'s
/// reference-white-preserving shoulder and gamut mapping, where `legacy` applies
/// the print controls before a plain ICC transform.
pub fn render_sdr_preset(
    image: &LinearImage,
    film_base: &FilmBase,
    reconstruction: &Reconstruction,
    print: &PrintParams,
    gamut: sdr::SdrGamut,
) -> Result<Rendered> {
    // Before the source render, so an unusable knee width fails without having paid
    // for reconstruction and the print stage.
    let tone = DisplayTone::resolve(print)?;
    let source = render_display_source(image, film_base, reconstruction, print)?;
    let mut timings = source.timings;
    let started = Instant::now();
    let rendered = sdr::render(&source.shared, gamut, tone)?;
    // The transform into the output space is colour work, like the gain-map and
    // Rec.2100 branches; only the container write counts as encode.
    let (image, icc, _metadata) = color::encode_rendered_sdr(rendered)?;
    timings.color_ms += ms_since(started);
    Ok(Rendered {
        image,
        icc,
        convert: source.convert,
        timings,
    })
}

/// Reconstruct once, cross the NC film RGB v1 boundary, then resolve and apply
/// the shared print controls exactly once for both gain-map renditions.
pub fn render_display_source(
    image: &LinearImage,
    film_base: &FilmBase,
    reconstruction: &Reconstruction,
    print: &PrintParams,
) -> Result<DisplaySource> {
    let started = Instant::now();
    let (film, recon) = algo::reconstruct(image, film_base, reconstruction)?;
    let shared = render_split::display_source(working_space::map_nc_film_rgb_v1(film), print)?;
    let algorithm_ms = ms_since(started);

    Ok(DisplaySource {
        convert: ConvertReport {
            dmax: recon.dmax,
            curve_anchor: recon.curve_anchor,
            white_balance: Some(shared.controls.white_balance()),
            balance_range: recon.balance_range,
            out_of_table: recon.out_of_table,
        },
        shared,
        timings: StageTimings {
            algorithm_ms,
            color_ms: 0.0,
        },
    })
}

/// The frozen legacy no-preset path: reconstruct → print render → working→output
/// ICC transform — the pre-preset contract. `golden` pins the
/// [`reconstruct_and_print`] half's pixels bit-for-bit, and
/// `legacy_preset_render_is_the_frozen_reconstruct_print_colour_sequence` pins that
/// this function is still that sequence composed with `color::to_output`.
fn render_legacy(
    image: &LinearImage,
    film_base: &FilmBase,
    reconstruction: &Reconstruction,
    print: &PrintParams,
    output_params: &OutputParams,
) -> Result<Rendered> {
    let started = Instant::now();
    let (positive, convert) = reconstruct_and_print(image, film_base, reconstruction, print)?;
    let algorithm_ms = ms_since(started);

    // No copy here (`io/memory-preflight`): the pre-transform positive has no
    // consumer, so it is *moved* into `to_output`, which transforms those very
    // buffers and hands them back — one full-frame RGB buffer and one full-frame IR
    // plane less at peak than the clone this used to make.
    let started = Instant::now();
    let (image, icc) = color::to_output(positive, output_params)?;
    let color_ms = ms_since(started);

    Ok(Rendered {
        image,
        icc,
        convert,
        timings: StageTimings {
            algorithm_ms,
            color_ms,
        },
    })
}

/// The `film-master` branch: reconstruct → NC film RGB v1 → encode directly.
///
/// No print controls are consumed (they are validated all-default at the CLI
/// boundary before this runs — a requested adjustment is a loud error, never
/// silently dropped), no `color::to_output` transform runs (the pixels are
/// *already* linear ACEScg; transforming again would double-apply the matrix),
/// and nothing is clamped. The ICC blob is the ACEScg profile the values are
/// genuinely in, fetched without building a transform.
///
/// [`ConvertReport::white_balance`] stays `None` here by construction: no
/// white-balance stage ran, and reporting resolved gains for a master that
/// applied none would be a false provenance claim. The reconstruction's own
/// resolved diagnostics (`dmax`, `balance_range`) *are* reported — they are part
/// of what the master contains.
fn render_film_master(
    image: &LinearImage,
    film_base: &FilmBase,
    reconstruction: &Reconstruction,
) -> Result<Rendered> {
    let started = Instant::now();
    let (film, recon) = algo::reconstruct(image, film_base, reconstruction)?;
    let master = render_split::film_master(working_space::map_nc_film_rgb_v1(film));
    let algorithm_ms = ms_since(started);

    let started = Instant::now();
    // Profile only — no transform. `icc_profile` builds the same ACEScg profile
    // `to_output` would embed, so the tag matches the pixels exactly.
    let icc = color::icc_profile(&OutputSpace::AcesCg)?;
    let color_ms = ms_since(started);

    Ok(Rendered {
        image: master,
        icc,
        convert: ConvertReport {
            dmax: recon.dmax,
            curve_anchor: recon.curve_anchor,
            white_balance: None,
            balance_range: recon.balance_range,
            out_of_table: recon.out_of_table,
        },
        timings: StageTimings {
            algorithm_ms,
            color_ms,
        },
    })
}

/// Wall-clock durations of the two in-memory stages [`render`] runs, in
/// milliseconds. Report-only diagnostics the orchestrator folds into the
/// telemetry record alongside its own decode / film-base / encode timings.
#[derive(Clone, Copy, Debug, Default)]
pub struct StageTimings {
    pub algorithm_ms: f64,
    pub color_ms: f64,
}

/// Milliseconds elapsed since `started`, as an `f64` for the telemetry record.
fn ms_since(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use crate::types::OutDepth;

    /// `OutputParams::default()` pinned to the **legacy** preset.
    ///
    /// The product default is `gain-map-hdr` since the `output/presets` migration,
    /// and `render` refuses it by design (display presets go through
    /// `render_display_source`). Any test calling `render` therefore has to say
    /// which TIFF branch it means.
    fn legacy_output() -> OutputParams {
        OutputParams {
            preset: OutputPreset::Legacy,
            ..OutputParams::default()
        }
    }
    use super::*;
    use crate::pipeline::film_base;
    use crate::types::{
        DensityCurve, DensityParams, DmaxSource, ExponentialParams, FilmBaseSource, SigmoidParams,
    };

    /// A small synthetic negative with the real scan layout — a near-black
    /// holder ring, then a bright, uniform orange rebate band (the film base),
    /// then a varied interior — so `Auto` estimation has a rebate to find.
    fn synthetic_negative(w: u32, h: u32) -> LinearImage {
        let holder = [0.01, 0.01, 0.01];
        let rebate = [0.9, 0.55, 0.42];
        let (holder_px, rebate_px) = (1, 2);
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let depth = x.min(y).min(w - 1 - x).min(h - 1 - y);
                if depth < holder_px {
                    rgb.extend_from_slice(&holder);
                } else if depth < holder_px + rebate_px {
                    rgb.extend_from_slice(&rebate);
                } else {
                    // Varied picture content, darker than the rebate.
                    let t = (x + y) as f32 / (w + h) as f32;
                    rgb.extend_from_slice(&[0.1 + 0.3 * t, 0.08 + 0.2 * t, 0.05 + 0.15 * t]);
                }
            }
        }
        LinearImage::new(w, h, rgb, None).unwrap()
    }

    fn density_default() -> Reconstruction {
        Reconstruction::default()
    }

    /// A reconstruction that lands this module's synthetic pixels in the **middle**
    /// of the range, where a transfer function visibly changes a value.
    ///
    /// The default (sigmoid at the nominal 1.3 anchor) renders these vectors to
    /// ~0.999, and near white the sRGB transfer and the linear value converge — so
    /// a test asking "was the transfer applied?" stops being able to tell. That is
    /// a property of the fixture, not of the branch under test, so the fixture is
    /// pinned rather than the assertion loosened.
    fn midtone_reconstruction() -> Reconstruction {
        Reconstruction::Density {
            density: DensityParams::default(),
            curve: DensityCurve::Exponential(ExponentialParams {
                gamma: 1.0,
                dmax: DmaxSource::Explicit(2.0),
                anchor: crate::types::AnchorPlacement::WhiteAtDmax,
            }),
        }
    }

    fn sigmoid_default() -> Reconstruction {
        Reconstruction::Density {
            density: DensityParams::default(),
            curve: DensityCurve::Sigmoid(SigmoidParams::default()),
        }
    }

    /// The characteristic curve on the generic profile — the fourth producer that has to
    /// reach the master through the same mapper.
    fn characteristic_default() -> Reconstruction {
        Reconstruction::Density {
            density: DensityParams::default(),
            curve: DensityCurve::Characteristic(crate::types::CharacteristicParams::default()),
        }
    }

    /// Resolve the film base the way the orchestrator does (stage 2), so the
    /// render tests exercise the same estimate → render sequence as `cli`.
    fn resolve(img: &LinearImage, source: FilmBaseSource) -> FilmBase {
        film_base::estimate(img, &source).unwrap().base
    }

    #[test]
    fn render_runs_the_full_simple_path_and_transforms_color() {
        let img = synthetic_negative(40, 40);
        // The auto estimate lands on the bright orange base (r > b).
        let base = resolve(&img, FilmBaseSource::Auto);
        assert!(base.r > base.b, "orange base: r > b");
        let out = render(
            &img,
            &base,
            &Reconstruction::Simple,
            &PrintParams::default(),
            &legacy_output(),
        )
        .unwrap();
        assert_eq!((out.image.width, out.image.height), (40, 40));
        assert!(!out.icc.is_empty(), "an ICC profile must be produced");
        // Simple has no curve or print stage to report.
        assert_eq!(out.convert, ConvertReport::default());
    }

    #[test]
    fn render_runs_the_density_path_with_explicit_base() {
        let img = synthetic_negative(16, 16);
        let base = FilmBase::from([0.9, 0.55, 0.42]);
        let out = render(
            &img,
            &base,
            &density_default(),
            &PrintParams::default(),
            &OutputParams {
                preset: OutputPreset::Legacy,
                depth: OutDepth::F32,
                ..OutputParams::default()
            },
        )
        .unwrap();
        assert_eq!(out.image.rgb.len(), 16 * 16 * 3);
    }

    #[test]
    fn render_runs_the_sigmoid_path_and_reports_the_anchor() {
        let img = synthetic_negative(16, 16);
        let base = FilmBase::from([0.9, 0.55, 0.42]);
        let out = render(
            &img,
            &base,
            &sigmoid_default(),
            &PrintParams::default(),
            &OutputParams {
                preset: OutputPreset::Legacy,
                depth: OutDepth::F32,
                ..OutputParams::default()
            },
        )
        .unwrap();
        assert_eq!(out.image.rgb.len(), 16 * 16 * 3);
        // The default Fixed anchor rides back through ConvertReport.
        assert!(out.convert.dmax.is_some_and(f32::is_finite));
    }

    #[test]
    fn legacy_preset_render_is_the_frozen_reconstruct_print_colour_sequence() {
        // `golden` pins `reconstruct_and_print`'s pixels, but it calls that helper
        // *directly* — it never crosses `render`'s `match output_params.preset`. So
        // swapping the two match arms would leave every golden green, and the
        // legacy-vs-legacy e2e comparison (implicit vs explicit `--output-preset
        // legacy`) would stay byte-identical too. Pin the boundary itself: the
        // no-preset render must BE `color::to_output(reconstruct_and_print(…))`,
        // bit-for-bit, image and ICC.
        //
        // In-process, so both sides come from this build's lcms2 and this is not the
        // cross-target ICC/post-transform trap.
        let img = synthetic_negative(8, 8);
        let base = FilmBase::from([0.9, 0.55, 0.42]);
        let print = PrintParams {
            print_exposure: -0.5,
            black_point: 0.01,
            ..PrintParams::default()
        };
        for reconstruction in [
            Reconstruction::Simple,
            density_default(),
            sigmoid_default(),
            characteristic_default(),
        ] {
            for output in [
                legacy_output(),
                OutputParams {
                    preset: OutputPreset::Legacy,
                    depth: OutDepth::F32,
                    ..OutputParams::default()
                },
                OutputParams {
                    preset: OutputPreset::Legacy,
                    output_profile: Some("srgb".into()),
                    ..OutputParams::default()
                },
            ] {
                let got = render(&img, &base, &reconstruction, &print, &output).unwrap();
                let (positive, convert) =
                    reconstruct_and_print(&img, &base, &reconstruction, &print).unwrap();
                let (want_image, want_icc) = color::to_output(positive, &output).unwrap();
                let bits = |v: &[f32]| -> Vec<u32> { v.iter().map(|x| x.to_bits()).collect() };
                assert_eq!(
                    bits(&got.image.rgb),
                    bits(&want_image.rgb),
                    "{reconstruction:?} / {output:?}"
                );
                assert_eq!(
                    got.image.ir, want_image.ir,
                    "{reconstruction:?} / {output:?}"
                );
                assert_eq!(got.icc, want_icc, "{reconstruction:?} / {output:?}");
                assert_eq!(got.convert, convert, "{reconstruction:?} / {output:?}");
            }
        }
    }

    #[test]
    fn film_master_render_bypasses_the_colour_transform_and_print_controls() {
        // The film-master branch must hand back the *mapped* ACEScg pixels: no
        // print render, and no `color::to_output` transform (which would
        // re-apply the Rec.709→ACEScg matrix on values that already crossed it).
        // Pin that by recomputing the expected buffer through the mapper directly.
        let img = synthetic_negative(8, 8);
        let base = FilmBase::from([0.9, 0.55, 0.42]);
        let out = render(
            &img,
            &base,
            &density_default(),
            // Non-default print controls are a *usage error* under film-master
            // (`cli::validate`); a defaulted set proves the branch simply never
            // consults them.
            &PrintParams::default(),
            &OutputParams {
                preset: OutputPreset::FilmMaster,
                ..OutputParams::default()
            },
        )
        .unwrap();

        let (film, _) = algo::reconstruct(&img, &base, &density_default()).unwrap();
        let want = working_space::map_nc_film_rgb_v1(film).into_linear();
        let bits = |v: &[f32]| -> Vec<u32> { v.iter().map(|x| x.to_bits()).collect() };
        assert_eq!(bits(&out.image.rgb), bits(&want.rgb));
        // The embedded tag is the *linear* ACEScg profile the values are genuinely
        // in — byte-identical to what `icc_profile` builds, so no display profile
        // and no transfer curve was substituted for the master. (Local-only byte
        // comparison: both sides come from the same lcms2 build in this process, so
        // this is not the cross-target ICC-bytes trap.)
        assert_eq!(out.icc, color::icc_profile(&OutputSpace::AcesCg).unwrap());
        // The master applied no white balance, so it claims none…
        assert_eq!(out.convert.white_balance, None);
        // …but the reconstruction's own resolved anchor IS part of the master.
        assert_eq!(out.convert.dmax, Some(crate::algo::density::NOMINAL_DMAX));
    }

    #[test]
    fn no_tone_gamut_or_transfer_operation_runs_on_film_master() {
        // The branch fixture for "`film-master` bypasses display rendering": run the
        // *same* inputs down both branches and show the legacy branch's operations
        // are observable while the master's pixels stay the mapper's output.
        let img = synthetic_negative(8, 8);
        let base = FilmBase::from([0.9, 0.55, 0.42]);
        let master_params = OutputParams {
            preset: OutputPreset::FilmMaster,
            ..OutputParams::default()
        };
        let (film, _) = algo::reconstruct(&img, &base, &midtone_reconstruction()).unwrap();
        let mapped = working_space::map_nc_film_rgb_v1(film).into_linear();

        // (a) A print control the legacy branch honours (2^1 exposure doubles every
        //     sample) leaves the master untouched. `render` is a pure function, so it
        //     is callable with the combination `cli::validate` rejects — which is
        //     exactly what makes this a bypass proof rather than a validation proof.
        let hot = PrintParams {
            print_exposure: 1.0,
            ..PrintParams::default()
        };
        let master = render(&img, &base, &midtone_reconstruction(), &hot, &master_params).unwrap();
        let bits = |v: &[f32]| -> Vec<u32> { v.iter().map(|x| x.to_bits()).collect() };
        assert_eq!(
            bits(&master.image.rgb),
            bits(&mapped.rgb),
            "a print control must not reach the master"
        );
        let legacy_hot = render(
            &img,
            &base,
            &midtone_reconstruction(),
            &hot,
            &OutputParams {
                preset: OutputPreset::Legacy,
                depth: OutDepth::F32,
                ..OutputParams::default()
            },
        )
        .unwrap();
        // `master * 1.5` is only a meaningful bar if the master sample is positive —
        // post-matrix ACEScg legitimately goes negative, and a `<= 0` master would
        // make the comparison trivially true.
        assert!(
            master.image.rgb[0] > 0.0,
            "the fixture's first master sample must be positive for the bound below \
             to mean anything (got {})",
            master.image.rgb[0]
        );
        assert!(
            legacy_hot.image.rgb[0] > master.image.rgb[0] * 1.5,
            "the legacy branch must actually apply the exposure it was given \
             (legacy {} vs master {})",
            legacy_hot.image.rgb[0],
            master.image.rgb[0]
        );

        // (b) A transfer/gamut operation the legacy branch honours (`srgb`, whose ICC
        //     carries the piecewise sRGB TRC) also leaves the master untouched: the
        //     master's samples stay linear, so a mid value must be visibly lower than
        //     the display-encoded one.
        let legacy_srgb = render(
            &img,
            &base,
            &midtone_reconstruction(),
            &PrintParams::default(),
            &OutputParams {
                preset: OutputPreset::Legacy,
                output_profile: Some("srgb".into()),
                ..OutputParams::default()
            },
        )
        .unwrap();
        let master_plain = render(
            &img,
            &base,
            &midtone_reconstruction(),
            &PrintParams::default(),
            &master_params,
        )
        .unwrap();
        assert!(
            legacy_srgb.image.rgb[0] > master_plain.image.rgb[0] + 0.05,
            "the legacy branch must apply the sRGB transfer it was given \
             (legacy {} vs master {})",
            legacy_srgb.image.rgb[0],
            master_plain.image.rgb[0]
        );
    }

    #[test]
    fn film_master_render_ignores_the_output_depth_knob_entirely() {
        // `film-master` resolves f32 by definition, not via `output.depth`. The depth
        // half is pinned in `types`; what belongs *here* is that `render` itself does
        // not consult the knob — change it and the master's pixels and ICC must be
        // bit-identical.
        // (`render` is pure, so it is callable with the combination `cli::validate`
        // rejects, which is what makes this a bypass proof.)
        let img = synthetic_negative(8, 8);
        let base = FilmBase::from([0.9, 0.55, 0.42]);
        let render_with = |depth: OutDepth| {
            render(
                &img,
                &base,
                &density_default(),
                &PrintParams::default(),
                &OutputParams {
                    preset: OutputPreset::FilmMaster,
                    depth,
                    ..OutputParams::default()
                },
            )
            .unwrap()
        };
        let (off, on) = (render_with(OutDepth::U16), render_with(OutDepth::F32));
        let bits = |v: &[f32]| -> Vec<u32> { v.iter().map(|x| x.to_bits()).collect() };
        assert_eq!(bits(&off.image.rgb), bits(&on.image.rgb));
        assert_eq!(off.icc, on.icc);
        // …and both resolve the f32 encode depth the branch is defined by.
        for depth in [OutDepth::U16, OutDepth::F32] {
            assert_eq!(
                OutputParams {
                    preset: OutputPreset::FilmMaster,
                    depth,
                    ..OutputParams::default()
                }
                .depth(),
                crate::types::OutDepth::F32
            );
        }
    }

    #[test]
    fn film_master_render_works_for_every_reconstruction_path() {
        // The split is producer-agnostic: simple, exponential, and sigmoid all
        // reach the master through the same mapper.
        let img = synthetic_negative(8, 8);
        let base = FilmBase::from([0.9, 0.55, 0.42]);
        let params = OutputParams {
            preset: OutputPreset::FilmMaster,
            ..OutputParams::default()
        };
        for reconstruction in [Reconstruction::Simple, density_default(), sigmoid_default()] {
            let out = render(
                &img,
                &base,
                &reconstruction,
                &PrintParams::default(),
                &params,
            )
            .unwrap();
            assert_eq!(out.image.rgb.len(), 8 * 8 * 3, "{reconstruction:?}");
            assert_eq!(out.convert.white_balance, None, "{reconstruction:?}");
        }
    }

    #[test]
    fn render_rejects_a_degenerate_base() {
        // Defense-in-depth: even if a zero-channel base reached `render` (estimate
        // now rejects it at birth), the reconstruction must reject it rather than
        // divide by zero — exit 1, never a silently-wrong image.
        let img = synthetic_negative(20, 20);
        let base = FilmBase::from([0.0, 0.55, 0.42]);
        match render(
            &img,
            &base,
            &density_default(),
            &PrintParams::default(),
            &legacy_output(),
        ) {
            Err(e) => assert_eq!(e.exit_code(), 1),
            Ok(_) => panic!("expected a degenerate-base error"),
        }
    }
}

/// Where a *known* tone lands in the delivered image, per display tone.
///
/// The question this answers: when a render looks dark, is the reconstruction placing
/// midtones low, or is the display operator costing brightness? Those have different fixes
/// — one belongs to the curve, the other to the render — and reasoning about it from the
/// picture cannot separate them.
///
/// Synthetic and self-contained, so it runs in CI with no assets: a uniform frame is built
/// at exactly the density the stock's datasheet calls mid-grey, pushed through the real
/// reconstruction and display stages, and the delivered value read back.
#[cfg(test)]
mod midtone_placement {
    use super::*;
    use crate::algo::film_stock;
    use crate::pipeline::display_tone::{DisplayTone, Headroom, KneeWidth};
    use crate::pipeline::sdr::{self, SdrGamut};
    use crate::types::{
        CharacteristicParams, DensityParams, FilmBase, FilmStock, PrintParams, Reconstruction,
    };

    /// A uniform frame whose corrected density is exactly `d_prime` on every channel's own
    /// scale — i.e. the film's own record of a neutral tone at that density above base.
    fn uniform_at(d_prime_of: impl Fn(usize) -> f32) -> (LinearImage, FilmBase) {
        let base = FilmBase::from([0.5, 0.25, 0.15]);
        let b = [base.r, base.g, base.b];
        // D' = -log10(scan / base)  =>  scan = base * 10^(-D')
        let px: Vec<f32> = (0..3).map(|c| b[c] * 10f32.powf(-d_prime_of(c))).collect();
        let mut rgb = Vec::new();
        for _ in 0..16 {
            rgb.extend_from_slice(&px);
        }
        (LinearImage::new(4, 4, rgb, None).unwrap(), base)
    }

    /// The datasheet mid-grey patch, per channel, for one stock.
    fn mid_grey_patch(stock: FilmStock) -> (LinearImage, FilmBase) {
        let sc = film_stock::curves_for(stock);
        let [grey, _] = sc.aims.expect("a stock with a published aim table");
        let d_min = sc.d_min.expect("a measured stock");
        uniform_at(|c| {
            let log_e = {
                let t = sc.channels[0];
                let target = grey - d_min[0];
                let mut x = t[0].0;
                for w in t.windows(2) {
                    if (w[0].1 - target) * (w[1].1 - target) <= 0.0 && w[0].1 != w[1].1 {
                        x = w[0].0 + (target - w[0].1) / (w[1].1 - w[0].1) * (w[1].0 - w[0].0);
                        break;
                    }
                }
                x
            };
            let t = sc.channels[c];
            let i = t.partition_point(|p| p.0 <= log_e).clamp(1, t.len() - 1);
            let ((x0, d0), (x1, d1)) = (t[i - 1], t[i]);
            d0 + (log_e - x0) * (d1 - d0) / (x1 - x0)
        })
    }

    /// Where that patch is delivered under an arbitrary reconstruction and tone. `None`
    /// when the renderer *refuses* the combination, which is itself a result worth seeing.
    fn delivered_by(
        reconstruction: &Reconstruction,
        stock: FilmStock,
        tone: DisplayTone,
        print: &PrintParams,
    ) -> Option<[f32; 3]> {
        let (image, base) = mid_grey_patch(stock);
        let shared = render_display_source(&image, &base, reconstruction, print).ok()?;
        let out = sdr::render(&shared.shared, SdrGamut::DisplayP3, tone).ok()?;
        let rgb = &out.image().rgb;
        Some([rgb[0], rgb[1], rgb[2]])
    }

    /// **Extended Reinhard is now free at 0.18 and only at 0.18.** The operator absorbs
    /// its own midtone cost by construction, so a reconstruction that lands mid-grey
    /// *where the operator is anchored* pays nothing — but the cost was always a property
    /// of the value received (`f(u)/v` shrinks as `v` rises), and it still is either side
    /// of the anchor: a brighter midtone pays, a darker one is lifted. This is what makes
    /// the fix belong in the operator rather than in a `--print-exposure` default, and
    /// also what limits it: it corrects the placement the reconstructions actually target,
    /// not every placement.
    ///
    /// Printed as a table rather than asserted per row: the sigmoid's placement depends on
    /// its anchor and the resolved reference, so pinning its exact value here would pin
    /// another task's defaults.
    #[test]
    fn reinhard_is_free_at_mid_grey_and_priced_either_side_of_it() {
        let stock = FilmStock::Portra400;
        let print = PrintParams::default();
        let reinhard = DisplayTone::ExtendedReinhard(Headroom::new(4.0).unwrap());
        // Resolve the per-channel gain from the curve, exactly as the recipe and CLI paths
        // do. Constructing `DensityParams::default()` beside a characteristic curve would
        // apply the parametric curves' calibration on top of one that already carries each
        // stock's per-channel response — the double-correction `default_scale_for`
        // documents, and the reason a table built that way reads ~0.06 stop dark on those
        // rows for no reason connected to what it is measuring.
        let density = |curve: crate::types::DensityCurve| Reconstruction::Density {
            density: DensityParams {
                scale: DensityParams::default_scale_for(curve.curve_type()),
                ..DensityParams::default()
            },
            curve,
        };
        let sigmoid = crate::types::SigmoidParams::default();
        let cases: [(&str, Reconstruction); 6] = [
            (
                "characteristic (portra-400)",
                density(crate::types::DensityCurve::Characteristic(
                    CharacteristicParams { stock },
                )),
            ),
            (
                "sigmoid, shipped defaults",
                density(crate::types::DensityCurve::Sigmoid(sigmoid)),
            ),
            (
                "sigmoid, shoulder 0",
                density(crate::types::DensityCurve::Sigmoid(
                    crate::types::SigmoidParams {
                        shoulder: 0.0,
                        ..sigmoid
                    },
                )),
            ),
            (
                "sigmoid, shoulder 0 + toe 0",
                density(crate::types::DensityCurve::Sigmoid(
                    crate::types::SigmoidParams {
                        shoulder: 0.0,
                        toe: 0.0,
                        ..sigmoid
                    },
                )),
            ),
            (
                "characteristic (generic-c41)",
                density(crate::types::DensityCurve::Characteristic(
                    CharacteristicParams {
                        stock: FilmStock::GenericC41,
                    },
                )),
            ),
            (
                "exponential, defaults",
                density(crate::types::DensityCurve::Exponential(
                    crate::types::ExponentialParams::default(),
                )),
            ),
        ];
        println!(
            "\n{:30}{:>10}{:>12}{:>12}{:>10}",
            "reconstruction", "no tone", "shoulder", "reinhard", "cost"
        );
        for (name, reconstruction) in cases {
            let none = delivered_by(&reconstruction, stock, DisplayTone::None, &print);
            let shoulder = delivered_by(
                &reconstruction,
                stock,
                DisplayTone::HermiteShoulder(KneeWidth::new(0.0).unwrap()),
                &print,
            );
            let rein = delivered_by(&reconstruction, stock, reinhard, &print);
            // Red: the tone channel, unaffected by `density.scale`'s per-channel gain.
            let cost = match (none, rein) {
                (Some(n), Some(r)) if r[0] > 0.0 => format!("{:.3}", (n[0] / r[0]).log2()),
                _ => "—".to_string(),
            };
            let fmt = |v: Option<[f32; 3]>| match v {
                Some(x) => format!("{:.4}", x[0]),
                None => "refused".to_string(),
            };
            println!(
                "{name:30}{:>10}{:>12}{:>12}{cost:>10}",
                fmt(none),
                fmt(shoulder),
                fmt(rein)
            );
        }
        // The invariant worth pinning: free at the anchor, and monotone in the value it
        // is handed — so the sign of the cost tells you which side of 0.18 a
        // reconstruction landed on.
        let cost_at =
            |v: f32| -(crate::pipeline::display_tone::extended_reinhard(v, 16.0) / v).log2();
        assert!(
            cost_at(0.18).abs() < 1e-4,
            "not free at 0.18: {:.5}",
            cost_at(0.18)
        );
        assert!(
            cost_at(0.30) > cost_at(0.18) && cost_at(0.10) < cost_at(0.18),
            "cost should rise through the anchor: {:.3}, {:.3}, {:.3}",
            cost_at(0.10),
            cost_at(0.18),
            cost_at(0.30)
        );
    }

    fn delivered(stock: FilmStock, tone: DisplayTone, print: &PrintParams) -> [f32; 3] {
        let sc = film_stock::curves_for(stock);
        let [grey, _] = sc.aims.expect("a stock with a published aim table");
        let d_min = sc.d_min.expect("a measured stock");
        // The datasheet's own mid-grey, per channel: the grey aim is red-only, so the other
        // layers are read at the exposure that puts red there — which is what a neutral is.
        let (image, base) = uniform_at(|c| {
            let log_e = {
                let t = sc.channels[0];
                let target = grey - d_min[0];
                let mut x = t[0].0;
                for w in t.windows(2) {
                    if (w[0].1 - target) * (w[1].1 - target) <= 0.0 && w[0].1 != w[1].1 {
                        x = w[0].0 + (target - w[0].1) / (w[1].1 - w[0].1) * (w[1].0 - w[0].0);
                        break;
                    }
                }
                x
            };
            let t = sc.channels[c];
            let i = t.partition_point(|p| p.0 <= log_e).clamp(1, t.len() - 1);
            let ((x0, d0), (x1, d1)) = (t[i - 1], t[i]);
            d0 + (log_e - x0) * (d1 - d0) / (x1 - x0)
        });
        // **Identity per-channel gain, deliberately.** These are tests of the *display
        // operator*, and they need a reconstruction that puts mid-grey exactly at 0.18 —
        // extended Reinhard is free at 0.18 and only there, so measuring its cost anywhere
        // else measures the offset instead. The default gain `[1, 0.90, 0.86]` moves this
        // patch off 0.18 (it is calibrated for the sigmoid, which has no per-channel film
        // model; the characteristic curve already has one). That effect is the subject of
        // `the_default_gain_shifts_per_channel_level_on_both_curves`, not of these.
        let reconstruction = Reconstruction::Density {
            density: DensityParams {
                scale: [1.0, 1.0, 1.0],
                ..DensityParams::default()
            },
            curve: crate::types::DensityCurve::Characteristic(CharacteristicParams { stock }),
        };
        let shared = render_display_source(&image, &base, &reconstruction, print).unwrap();
        let out = sdr::render(&shared.shared, SdrGamut::DisplayP3, tone).unwrap();
        // **Red, not green.** The patch is uniform, so any channel reads the same *tone* —
        // but only red's `density.scale` gain is 1, so only red measures the tone question
        // this module is about without the per-channel calibration folded in. The colour
        // consequence of that calibration on this path is a separate assertion below.
        let rgb = &out.image().rgb;
        [rgb[0], rgb[1], rgb[2]]
    }

    /// **Mid-grey survives the whole pipeline, and this is the test that located the
    /// problem it now guards.** A datasheet mid-grey comes out of the curve at 0.18 by
    /// construction, so the ≈0.24 stop the render used to lose was never the
    /// reconstruction's — it was extended Reinhard's, on *every* curve. That is why the
    /// fix went into the operator's own definition rather than into a default
    /// `--print-exposure`, and this asserts the end-to-end result: 0.18 in, 0.18
    /// delivered, under every tone and at every headroom.
    #[test]
    fn mid_grey_lands_at_eighteen_percent_through_every_display_tone() {
        let stock = FilmStock::Portra400;
        let print = PrintParams::default();
        // Red only: see `delivered`. Tone is the question here.
        let tone_of = |t, p: &PrintParams| delivered(stock, t, p)[0];

        // No tone: gamut mapping and the range check still run, but nothing reshapes tone.
        let none = tone_of(DisplayTone::None, &print);
        assert!(
            (none - 0.18).abs() < 0.01,
            "reconstruction placed the datasheet's mid-grey at {none:.4}, not 0.18 — the \
             darkness would then be the curve's, not the operator's"
        );

        // Extended Reinhard: free at mid-grey, at every headroom. Swept rather than
        // spot-checked because the gain is a function of the white point, so "independent
        // of the headroom" is the actual claim.
        let reinhard = tone_of(
            DisplayTone::ExtendedReinhard(Headroom::new(4.0).unwrap()),
            &print,
        );
        let stops = (none / reinhard).log2();
        assert!(
            stops.abs() < 0.02,
            "extended Reinhard cost {stops:.3} stop at mid-grey, expected none"
        );
        for headroom in [0.0f32, 1.0, 4.0, 6.0, 10.0] {
            let at = tone_of(
                DisplayTone::ExtendedReinhard(Headroom::new(headroom).unwrap()),
                &print,
            );
            assert!(
                (at - 0.18).abs() < 0.005,
                "{headroom} stops of headroom delivered {at:.4}, not 0.18"
            );
        }

        // The shipped shoulder tone: a knee placed well above mid-grey, so it never touched
        // this in the first place — which is what made the loss diagnosable as the
        // operator's rather than the pipeline's.
        let shoulder = tone_of(
            DisplayTone::HermiteShoulder(KneeWidth::new(0.0).unwrap()),
            &print,
        );
        assert!(
            (shoulder - 0.18).abs() < 0.01,
            "the shoulder delivered {shoulder:.4}, not 0.18"
        );
        println!(
            "mid-grey delivered (red) — none {none:.4}, shoulder {shoulder:.4}, \
             reinhard {reinhard:.4} ({stops:.3} stop)"
        );
    }

    /// **The `print_exposure` each candidate look needs to deliver one common mid-grey.**
    ///
    /// The looks pair a reconstruction with a display tone, and each pairing lands a true
    /// mid-grey somewhere different — the sigmoid's anchor places it by its own rule while
    /// the characteristic curve reads it off the film, and the two tones then treat it
    /// differently again. Matching them is what makes the looks comparable by eye instead
    /// of "one is brighter".
    ///
    /// Printed rather than asserted per row: the target is a **taste** (the user's approved
    /// `+0.31` over scene mid-grey), so pinning each value here would pin a preference in a
    /// test. What *is* asserted is that the spread between looks is real — if it were
    /// noise, one number would serve them all and the looks would not need their own.
    #[test]
    fn each_candidate_look_needs_its_own_print_exposure() {
        let print = PrintParams::default();
        let sigmoid = crate::types::SigmoidParams::default();
        let reinhard = DisplayTone::ExtendedReinhard(Headroom::new(6.0).unwrap());
        let density = |curve: crate::types::DensityCurve| Reconstruction::Density {
            density: DensityParams {
                scale: DensityParams::default_scale_for(curve.curve_type()),
                ..DensityParams::default()
            },
            curve,
        };
        let stock = FilmStock::Portra400;
        // The approved look: scene mid-grey rendered 0.31 stop up.
        let target = 0.18 * 2f32.powf(0.31);

        let looks: [(&str, Reconstruction, DisplayTone); 4] = [
            (
                "characteristic-stock + reinhard",
                density(crate::types::DensityCurve::Characteristic(
                    CharacteristicParams { stock },
                )),
                reinhard,
            ),
            (
                "characteristic-generic + reinhard",
                density(crate::types::DensityCurve::Characteristic(
                    CharacteristicParams {
                        stock: FilmStock::GenericC41,
                    },
                )),
                reinhard,
            ),
            (
                "sigmoid knees + linear (none)",
                density(crate::types::DensityCurve::Sigmoid(sigmoid)),
                DisplayTone::None,
            ),
            (
                "sigmoid flat + reinhard",
                density(crate::types::DensityCurve::Sigmoid(
                    crate::types::SigmoidParams {
                        toe: 0.0,
                        shoulder: 0.0,
                        ..sigmoid
                    },
                )),
                reinhard,
            ),
        ];
        println!(
            "\n  target: scene mid-grey 0.18 rendered at {target:.4} (+0.31 stop)\n\n  \
             {:34}{:>11}{:>16}",
            "look", "delivered", "needs exposure"
        );
        let mut needed = vec![];
        for (name, reconstruction, tone) in looks {
            let Some(rgb) = delivered_by(&reconstruction, stock, tone, &print) else {
                println!("  {name:34}    refused");
                continue;
            };
            let stops = (target / rgb[0]).log2();
            needed.push(stops);
            println!("  {name:34}{:>11.4}{:>+16.3}", rgb[0], stops);
        }
        let (lo, hi) = (
            needed.iter().cloned().fold(f32::MAX, f32::min),
            needed.iter().cloned().fold(f32::MIN, f32::max),
        );
        assert!(
            hi - lo > 0.25,
            "the looks needed exposures within {:.3} stop of each other — if that is real, \
             one default would serve them all and a per-look value is unjustified",
            hi - lo
        );
    }

    /// **`sigmoid-knees` + no display tone cannot be brightened with `--print-exposure`,
    /// so its brightness has to come from the anchor.**
    ///
    /// `--display-tone none` relies on the reconstruction being bounded at the render's own
    /// ceiling — that is what makes the mode self-policing. `--print-exposure` is a scalar
    /// gain applied after the curve, so *any* positive value pushes the shoulder's output
    /// past reference white and the range check refuses the frame (measured on a real
    /// Ektar scan at `+0.70`: "pixel 0 sits above reference white (luminance 1.6236)",
    /// which is exactly `2^0.70`). The two knobs are incompatible by construction, not by
    /// accident.
    ///
    /// The anchor is the knob that works: it moves mid-grey *within* the curve's bounded
    /// range instead of scaling the range. This sweeps it to find the placement that lands
    /// the shared target, and asserts the result stays renderable under `none`.
    #[test]
    fn the_linear_rendered_sigmoid_takes_its_brightness_from_the_anchor() {
        let print = PrintParams::default();
        let sigmoid = crate::types::SigmoidParams::default();
        let target = 0.18 * 2f32.powf(0.31);
        let at_fraction = |f: f32| {
            let curve = crate::types::DensityCurve::Sigmoid(crate::types::SigmoidParams {
                anchor: crate::types::AnchorPlacement::MidAtDmaxFraction(f),
                ..sigmoid
            });
            delivered_by(
                &Reconstruction::Density {
                    density: DensityParams {
                        scale: DensityParams::default_scale_for(curve.curve_type()),
                        ..DensityParams::default()
                    },
                    curve,
                },
                FilmStock::Portra400,
                DisplayTone::None,
                &print,
            )
            .map(|rgb| rgb[0])
        };
        println!(
            "\n  target {target:.4} — mid-grey delivered by the shouldered sigmoid under \
             `none`,\n  as the anchor fraction moves (default is 0.5):\n"
        );
        let mut best = (f32::MAX, 0.0f32, 0.0f32);
        for step in 0..=14 {
            let f = 0.20 + 0.02 * step as f32;
            let Some(v) = at_fraction(f) else {
                println!("    fraction {f:.2}  refused");
                continue;
            };
            let err = (v / target).log2().abs();
            if err < best.0 {
                best = (err, f, v);
            }
            println!(
                "    fraction {f:.2}  delivers {v:.4}  ({:+.3} stop)",
                (v / target).log2()
            );
        }
        let (err, f, v) = best;
        println!("\n  closest: fraction {f:.2} delivers {v:.4} ({err:.3} stop off target)");
        assert!(
            err < 0.05,
            "no anchor fraction in 0.20..=0.48 lands the shared target; closest was \
             {f:.2} at {v:.4}, {err:.3} stop off"
        );
        // ...and lowering the fraction is what brightens, which is the direction a preset
        // has to encode. Asserted so a sign flip in `AnchorPlacement` is caught here.
        let (lo, hi) = (at_fraction(0.30).unwrap(), at_fraction(0.50).unwrap());
        assert!(
            lo > hi,
            "a lower anchor fraction should deliver a brighter mid-grey: {lo:.4} vs {hi:.4}"
        );
    }

    /// How much per-channel *level* does the default gain move, on each curve?
    ///
    /// Checked because an earlier write-up claimed "≤0.06 stop, the anchoring absorbs it",
    /// measured from whole-image means through `nc convert`. That measurement was bad: two
    /// of the three frames sat near clipping, where the mean cannot move. On a mid-grey
    /// patch the shift is an order of magnitude larger, and it is a **level**, not the
    /// tilt the drift probes measure — so it is what actually decides whether the render
    /// reads magenta.
    #[test]
    fn the_default_gain_shifts_per_channel_level_on_both_curves() {
        let stock = FilmStock::Portra400;
        let print = PrintParams::default();
        let sigmoid = crate::types::SigmoidParams::default();
        let identity = DensityParams {
            scale: [1.0, 1.0, 1.0],
            ..DensityParams::default()
        };
        let cases: [(&str, crate::types::DensityCurve); 2] = [
            (
                "characteristic",
                crate::types::DensityCurve::Characteristic(CharacteristicParams { stock }),
            ),
            ("sigmoid", crate::types::DensityCurve::Sigmoid(sigmoid)),
        ];
        println!(
            "\n  {:16}{:>10}{:>10}{:>10}   per-channel stops vs identity",
            "curve", "R", "G", "B"
        );
        let mut worst = 0.0f32;
        for (name, curve) in cases {
            let at = |density: DensityParams| {
                let reconstruction = Reconstruction::Density { density, curve };
                delivered_by(&reconstruction, stock, DisplayTone::None, &print)
            };
            let (Some(base), Some(now)) = (at(identity.clone()), at(DensityParams::default()))
            else {
                continue;
            };
            let stops: Vec<f32> = (0..3).map(|c| (now[c] / base[c]).log2()).collect();
            worst = worst.max(stops.iter().fold(0.0f32, |a, b| a.max(b.abs())));
            println!(
                "  {:16}{:>+10.3}{:>+10.3}{:>+10.3}",
                name, stops[0], stops[1], stops[2]
            );
        }
        assert!(
            worst > 0.1,
            "the gain moved per-channel level by at most {worst:.3} stop — if that is \
             really so, the claim it is absorbed needs re-examining, not this assertion"
        );

        // The question that decides the default: on a patch that is neutral *in the
        // scene*, does the gain make the delivered pixel more neutral or less? The
        // datasheet mid-grey patch is exactly that patch — its three densities are the
        // published ones at the grey aim's exposure.
        println!(
            "\n  {:16}{:>12}{:>12}   delivered spread (max |dev| from the mean)",
            "curve", "identity", "default"
        );
        for (name, curve) in [
            (
                "characteristic",
                crate::types::DensityCurve::Characteristic(CharacteristicParams { stock }),
            ),
            ("sigmoid", crate::types::DensityCurve::Sigmoid(sigmoid)),
        ] {
            let at = |density: DensityParams| {
                delivered_by(
                    &Reconstruction::Density { density, curve },
                    stock,
                    DisplayTone::None,
                    &print,
                )
            };
            let spread = |rgb: [f32; 3]| {
                let m = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
                (0..3)
                    .map(|c| (rgb[c] - m) / m)
                    .fold(0.0f32, |a, b| a.max(b.abs()))
            };
            if let (Some(id), Some(def)) = (at(identity.clone()), at(DensityParams::default())) {
                println!(
                    "  {:16}{:>12.4}{:>12.4}   {:?} -> {:?}",
                    name,
                    spread(id),
                    spread(def),
                    id,
                    def
                );
            }
        }
    }
}

/// Golden fixtures pinning the reconstruction split bit-for-bit against the
/// **pre-split monolithic converters** (`Algorithm::{Simple,Density,Sigmoid}`).
/// Most expected values below were captured by running the pre-refactor code on
/// these exact inputs and printing `f32::to_bits` — so any arithmetic drift in the
/// split (a reordered multiply, a changed intermediate, a lost anchor) fails these
/// tests immediately, not in a downstream image diff. That was the split task's
/// acceptance gate: the split is a structural refactor, the default pixels are the
/// contract.
///
/// **Two of the twelve are not reference captures**, and the claim has to be scoped
/// or it stops being true. `golden_sigmoid_at_the_reference_anchor_is_numerically_exact`
/// was recaptured on 2026-08-03 when the sigmoid's own defaults deliberately moved, and
/// `golden_new_default_is_bit_identical` was captured fresh from this build on
/// 2026-08-08 for the new default render. Both honestly pin "this has not drifted
/// since it was set", which is strictly weaker than "matches the reference
/// implementation" — no golden can claim the stronger thing about a value that was
/// deliberately changed. Each says so at its own call site; read it before treating
/// a failure there as proof of a regression. Nothing here hashes an encoded TIFF:
/// see the note at the end of the module for why that could never be checked in.
///
/// The module is (test-only) `pub(crate)` so the `pipeline_version` drift gate in
/// [`crate::version`] fingerprints **these exact** curated vectors instead of a
/// second copy that could quietly drift away from them.
#[cfg(test)]
pub(crate) mod golden {
    use super::*;
    use crate::algo::film_stock::{OutOfTable, curves_for, invert};
    use crate::types::{
        AnchorPlacement, BalanceRange, CharacteristicParams, DensityCurve, DensityCurveType,
        DensityParams, DmaxSource, ExponentialParams, SigmoidParams, WbSource,
    };

    /// Five pixels spanning the tonal range plus out-of-range finite values,
    /// with an IR plane (`[0.1, 0.2, 0.3, 0.4, 0.5]`):
    /// near-base shadow, midtone, dense highlight, out-of-range (above base /
    /// negative / zero → epsilon floor), and exactly-the-base.
    ///
    /// These five pixels are the crate's cross-platform bit-identity substrate:
    /// their default-path results are pinned here **and** hashed into the
    /// `pipeline_version` drift gate (`crate::version`), so both mechanisms measure
    /// the same thing.
    pub(crate) fn pixels() -> LinearImage {
        LinearImage::new(
            5,
            1,
            vec![
                0.85, 0.5, 0.38, // near-base shadow
                0.3, 0.18, 0.12, // midtone
                0.02, 0.012, 0.009, // dense highlight
                1.5, -0.2, 0.0, // out-of-range finite
                0.9, 0.55, 0.42, // exactly the base
            ],
            Some(vec![0.1, 0.2, 0.3, 0.4, 0.5]),
        )
        .unwrap()
    }

    /// The film base [`pixels`] is reconstructed against (shared with the drift
    /// gate, for the same reason).
    pub(crate) fn base() -> FilmBase {
        FilmBase::from([0.9, 0.55, 0.42])
    }

    /// The custom density-reconstruction block the customized cases share.
    fn custom_density() -> DensityParams {
        DensityParams {
            scale: [1.1, 1.0, 0.9],
            offset: [0.05, 0.0, -0.05],
            shadow_balance: [0.05, 0.0, -0.02],
            highlight_balance: [-0.05, 0.01, 0.0],
            balance_range: BalanceRange::Explicit([0.2, 1.6]),
        }
    }

    /// Non-neutral regional balances over otherwise-default density correction
    /// — the block the auto-WB/auto-range composition goldens were captured
    /// with (unlike [`custom_density`], scale/offset stay at their defaults).
    fn balanced_density() -> DensityParams {
        DensityParams {
            shadow_balance: [0.05, 0.0, -0.02],
            highlight_balance: [-0.05, 0.01, 0.0],
            balance_range: BalanceRange::Explicit([0.2, 1.6]),
            ..frozen_density()
        }
    }

    /// The custom print block the customized cases share.
    fn custom_print() -> PrintParams {
        PrintParams {
            print_exposure: -1.0,
            black_point: 0.01,
            white_balance: WbSource::Explicit([1.0, 1.05, 1.1]),
            highlight_compress: 0.2,
            ..PrintParams::default()
        }
    }

    /// Assert the pre-color-transform pixels (and the resolved diagnostics)
    /// match the captured pre-refactor bits exactly.
    fn assert_golden(
        reconstruction: Reconstruction,
        print: PrintParams,
        expected_rgb_bits: &[u32],
        expected_dmax_bits: Option<u32>,
        expected_wb_bits: Option<[u32; 3]>,
        expected_range_bits: Option<[u32; 2]>,
    ) {
        let (out, report) =
            reconstruct_and_print(&pixels(), &base(), &reconstruction, &print).unwrap();
        let got: Vec<u32> = out.rgb.iter().map(|v| v.to_bits()).collect();
        assert_eq!(got, expected_rgb_bits, "pixel bits drifted");
        assert_eq!(report.dmax.map(f32::to_bits), expected_dmax_bits, "dmax");
        assert_eq!(
            report.white_balance.map(|w| w.map(f32::to_bits)),
            expected_wb_bits,
            "white balance"
        );
        assert_eq!(
            report.balance_range.map(|r| r.map(f32::to_bits)),
            expected_range_bits,
            "balance range"
        );
        // IR rides through untouched on every path.
        assert_eq!(out.ir.as_deref(), Some(&[0.1f32, 0.2, 0.3, 0.4, 0.5][..]));
    }

    const UNIT_WB: [u32; 3] = [0x3f800000; 3]; // [1.0, 1.0, 1.0]

    /// The identity per-channel gain, for every capture that is **frozen** against a
    /// reference implementation or a past build rather than tracking the live default.
    ///
    /// `DensityParams::default()`'s gain became `[1, 0.90, 0.86]` in `pipeline_version` 4
    /// — a scanner calibration. A capture whose meaning is "this still agrees with the
    /// reference implementation's arithmetic" must not absorb that: recapturing it under a
    /// new default would keep the test green while silently deleting the agreement it
    /// exists to prove. Only `golden_new_default_is_bit_identical` tracks the default, and
    /// it was recaptured.
    fn frozen_density() -> DensityParams {
        DensityParams {
            scale: [1.0, 1.0, 1.0],
            ..DensityParams::default()
        }
    }

    /// The configuration the vectors below were captured under: the exponential
    /// straight line at gamma **1.0** with the anchor pinned at the old
    /// `NOMINAL_DMAX` of **2.0**.
    ///
    /// Pinned **explicitly** rather than through `DensityCurve::default()`, because
    /// on 2026-08-08 the default became the sigmoid, `NOMINAL_DMAX` became 1.3, and
    /// this curve's gamma became 2.0. A golden vector that silently follows the
    /// default stops pinning anything the moment the default moves — it just
    /// re-describes whatever the build now does. Naming the configuration keeps
    /// every bit below exactly as captured from the reference code, and the *new*
    /// default gets its own golden (`golden_new_default_...`).
    ///
    /// `Explicit(2.0)` is arithmetically identical to the old `Fixed`: both resolve
    /// to the same anchor value, so these are the original captures, not a rebase.
    fn frozen_reference_curve() -> DensityCurve {
        DensityCurve::Exponential(ExponentialParams {
            gamma: 1.0,
            dmax: DmaxSource::Explicit(2.0),
            anchor: AnchorPlacement::WhiteAtDmax,
        })
    }

    /// `Reconstruction::default()`'s density knobs with [`frozen_reference_curve`].
    fn frozen_reference_config() -> Reconstruction {
        Reconstruction::Density {
            density: frozen_density(),
            curve: frozen_reference_curve(),
        }
    }

    /// The defining property of `BlackAtBase`: the film base renders to exactly the
    /// stated floor, whatever the slope.
    ///
    /// Asserted as a property rather than as captured bits, because captured bits for a
    /// rule introduced in the same commit only re-describe the build. Pixel 5 of
    /// [`pixels`] *is* the base, so its corrected density is exactly 0 and its rendered
    /// value is the floor by construction — which is what makes this falsifiable: a sign
    /// error in `−log10(floor)/contrast` moves it immediately.
    #[test]
    fn black_at_base_renders_the_film_base_to_the_stated_floor() {
        for floor in [0.002f32, 0.005, 0.05] {
            for gamma in [1.0f32, 2.0, 2.5] {
                let reconstruction = Reconstruction::Density {
                    density: DensityParams::default(),
                    curve: DensityCurve::Exponential(ExponentialParams {
                        gamma,
                        dmax: DmaxSource::Fixed,
                        anchor: AnchorPlacement::BlackAtBase(floor),
                    }),
                };
                let (out, _) = reconstruct_and_print(
                    &pixels(),
                    &base(),
                    &reconstruction,
                    &PrintParams::default(),
                )
                .unwrap();
                // Pixel 5 (rgb offsets 12..15) is exactly the base.
                for c in 0..3 {
                    let got = out.rgb[12 + c];
                    assert!(
                        (got - floor).abs() <= floor * 1e-5,
                        "floor {floor} gamma {gamma} channel {c}: rendered {got}"
                    );
                }
            }
        }
    }

    /// `BlackAtBase` is not a new curve — it is the same straight line at a derived
    /// anchor, so it must be **bit-identical** to `WhiteAtDmax` at that anchor.
    ///
    /// This is what keeps the exponential usable as the debuggable reference: the two
    /// spellings cannot diverge. It also pins the anchors the 2026-08-03 candidate retest
    /// recorded (floor 0.002 → 1.349, 0.005 → 1.151 at contrast 2.0).
    #[test]
    fn black_at_base_equals_white_at_dmax_at_the_derived_anchor() {
        let gamma = 2.0f32;
        for floor in [0.002f32, 0.005] {
            let derived = -floor.log10() / gamma;
            let render = |anchor| {
                let reconstruction = Reconstruction::Density {
                    density: DensityParams::default(),
                    curve: DensityCurve::Exponential(ExponentialParams {
                        gamma,
                        dmax: if matches!(anchor, AnchorPlacement::WhiteAtDmax) {
                            DmaxSource::Explicit(derived)
                        } else {
                            DmaxSource::Fixed
                        },
                        anchor,
                    }),
                };
                let (out, _) = reconstruct_and_print(
                    &pixels(),
                    &base(),
                    &reconstruction,
                    &PrintParams::default(),
                )
                .unwrap();
                out.rgb.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            };
            assert_eq!(
                render(AnchorPlacement::BlackAtBase(floor)),
                render(AnchorPlacement::WhiteAtDmax),
                "floor {floor} (derived anchor {derived})"
            );
        }
    }

    #[test]
    fn golden_new_default_is_bit_identical() {
        // THE default path as of 2026-08-08: density reconstruction, **sigmoid**
        // curve with its derived contrast/shoulder, the nominal anchor at the new
        // NOMINAL_DMAX = 1.3, neutral print.
        //
        // Captured from THIS build, not from the reference implementation — so it
        // pins "the default has not drifted since it was set", which is what a
        // default golden can honestly claim after the default deliberately moved.
        // The reference-derived captures live in the `frozen_reference_*` goldens
        // above and are untouched — they now say `frozen_density()` explicitly, so a
        // future default gain cannot silently rewrite what they verify.
        //
        // RECAPTURED 2026-09-09 (`pipeline_version` 4): `density.scale` `[1, 1, 1]` →
        // `[1, 0.90, 0.86]`. **Red is bit-identical on all five vectors** — its gain is
        // still 1 — and only green and blue move, which is the shape a per-channel gain
        // should produce and a useful check that nothing else drifted with it. The
        // mid-tone vector moves most (green ×0.796, blue ×0.700) because the sigmoid's
        // slope is steepest there; the near-white vector barely moves (×0.991, ×0.983)
        // since it sits on the shoulder, and the two clamped/base vectors not at all.
        assert_golden(
            Reconstruction::default(),
            PrintParams::default(),
            &[
                0x3c23e35f, 0x3c2a9542, 0x3c2aa7fc, 0x3da066cb, 0x3d848c78, 0x3d9999d1, 0x3f7f12f0,
                0x3f7cc9ee, 0x3f7ae485, 0x3c05798b, 0x3f800000, 0x3f800000, 0x3c1928cc, 0x3c1928cc,
                0x3c1928cc,
            ],
            Some(0x3fa66666), // NOMINAL_DMAX = 1.3
            Some(UNIT_WB),
            None,
        );
    }

    #[test]
    fn golden_density_exponential_reference_is_bit_identical() {
        // The exponential straight line at gamma 1.0 / anchor 2.0 — what used to be
        // THE default path, and still the reference capture these bits came from.
        // No longer reached by `Reconstruction::default()` (see
        // `frozen_reference_curve`), so it is named here instead.
        assert_golden(
            frozen_reference_config(),
            PrintParams::default(),
            &[
                0x3c2d7a46, 0x3c343958, 0x3c35161a, 0x3cf5c28f, 0x3cfa4fa3, 0x3d0f5c2a, 0x3ee66668,
                0x3eeaaaab, 0x3eeeeef1, 0x3bc49ba7, 0x45abdfff, 0x45833ffb, 0x3c23d70a, 0x3c23d70a,
                0x3c23d70a,
            ],
            Some(0x40000000), // the anchor these bits were captured at
            Some(UNIT_WB),
            None,
        );
    }

    #[test]
    fn golden_density_exponential_customized_is_bit_identical() {
        // Every density knob non-default at once: scale/offset, regional balance
        // with an explicit range, gamma 1.4, an explicit anchor, and a full
        // custom print (exposure, black point, gains, soft-clip).
        assert_golden(
            Reconstruction::Density {
                density: custom_density(),
                curve: DensityCurve::Exponential(ExponentialParams {
                    gamma: 1.4,
                    dmax: DmaxSource::Explicit(1.8),
                    anchor: AnchorPlacement::WhiteAtDmax,
                }),
            },
            custom_print(),
            &[
                0xbbfd1875, 0xbc0627d2, 0xbc0b3486, 0x3a6a9290, 0xbb1d24fc, 0xbb670fb4, 0x3f055214,
                0x3eac5540, 0x3e2d3fe0, 0xbc18931f, 0x3f99999a, 0x3f99999a, 0xbc01b0a7, 0xbc09dd14,
                0xbc0e1fb5,
            ],
            Some(0x3fe66666),                           // 1.8
            Some([0x3f800000, 0x3f866666, 0x3f8ccccd]), // [1.0, 1.05, 1.1]
            Some([0x3e4ccccd, 0x3fcccccd]),             // [0.2, 1.6]
        );
    }

    #[test]
    fn golden_density_exponential_no_anchor_is_bit_identical() {
        // `dmax = none` — the scene-referred unity placement (base → 1.0).
        assert_golden(
            Reconstruction::Density {
                density: frozen_density(),
                curve: DensityCurve::Exponential(ExponentialParams {
                    gamma: 1.0,
                    dmax: DmaxSource::None,
                    anchor: AnchorPlacement::WhiteAtDmax,
                }),
            },
            PrintParams::default(),
            &[
                0x3f878787, 0x3f8ccccd, 0x3f8d7943, 0x403fffff, 0x40438e38, 0x40600000, 0x42340001,
                0x42375556, 0x423aaaac, 0x3f199999, 0x490646ff, 0x48cd13f9, 0x3f800000, 0x3f800000,
                0x3f800000,
            ],
            None,
            Some(UNIT_WB),
            None,
        );
    }

    #[test]
    fn golden_density_exponential_auto_anchor_is_bit_identical() {
        // `dmax = auto` — the demoted per-frame percentile measurement.
        assert_golden(
            Reconstruction::Density {
                density: frozen_density(),
                curve: DensityCurve::Exponential(ExponentialParams {
                    gamma: 1.0,
                    dmax: DmaxSource::Auto,
                    anchor: AnchorPlacement::WhiteAtDmax,
                }),
            },
            PrintParams::default(),
            &[
                0x3601318e, 0x360637c0, 0x3606dc23, 0x36b70634, 0x36ba69df, 0x36d58734, 0x38ab95cf,
                0x38aec33f, 0x38b1f0b1, 0x35926b56, 0x3f800000, 0x3f437da7, 0x35f40842, 0x35f40842,
                0x35f40842,
            ],
            Some(1085780237), // the measured per-frame anchor, captured verbatim
            Some(UNIT_WB),
            None,
        );
    }

    /// The sigmoid with its shipped parameters but the anchor pinned at the **old**
    /// `NOMINAL_DMAX` of 2.0, so the 2026-08-03 recapture below stays the captured
    /// bits rather than silently following `NOMINAL_DMAX` to 1.3.
    fn sigmoid_at_reference_anchor_2_0() -> Reconstruction {
        Reconstruction::Density {
            density: frozen_density(),
            curve: DensityCurve::Sigmoid(SigmoidParams {
                dmax: DmaxSource::Explicit(2.0),
                ..SigmoidParams::default()
            }),
        }
    }

    #[test]
    fn golden_sigmoid_at_the_reference_anchor_is_numerically_exact() {
        // RECAPTURED 2026-08-03 (`algo/reference-anchored-sigmoid`, Phase 4). The sigmoid
        // defaults changed deliberately: contrast 1.0 → REFERENCE_CONTRAST (≈2.07), shoulder
        // 0.2 → 0.6, and the anchor is now mid-grey at half the reference rather than white
        // at the reference. On this synthetic vector the base pixel moves 0.0115 → 0.00177
        // (≈28/255 → ≈6/255, i.e. an actual black — the defect this task was opened for) and
        // the dense highlight 0.448 → 0.946.
        //
        // NOTE this is **no longer the sigmoid default**, which is why the name says
        // "at the reference anchor" instead. It runs the shipped sigmoid parameters at
        // `Explicit(2.0)` — the anchor `NOMINAL_DMAX` carried when these bits were
        // captured — so the capture keeps pinning what it was captured for. The live
        // default is the same curve at `NOMINAL_DMAX = 1.3`, pinned separately by
        // `golden_new_default_is_bit_identical`. On a *measured* roll (reference ≈1.35)
        // mid-grey lands at ≈0.67, matching real mid-tones; the fallback constant is
        // `film-base/dmax-anchor-reliability`'s to settle and this vector is not
        // evidence about it.
        //
        // The drift-gate fingerprints do not move for *this* vector: it is not a default
        // path, so `version::PIPELINE_FINGERPRINTS` never hashed it. The bump that flipped
        // the default curve to the sigmoid is v2, recorded on 2026-08-08 by this same
        // change — not, as this note used to predict, deferred to `output/presets`.
        assert_golden(
            sigmoid_at_reference_anchor_2_0(),
            PrintParams::default(),
            &[
                0x3af793c5, 0x3b02af7d, 0x3b03a290, 0x3c7438ec, 0x3c7da965, 0x3ca7e975, 0x3f72198a,
                0x3f72e4fa, 0x3f73a232, 0x3ac99e34, 0x3f800000, 0x3f800000, 0x3ae75cf6, 0x3ae75cf6,
                0x3ae75cf6,
            ],
            Some(0x40000000),
            Some(UNIT_WB),
            None,
        );
    }

    #[test]
    fn golden_sigmoid_customized_is_numerically_exact() {
        // Custom knees + explicit anchor + the custom density/print blocks.
        assert_golden(
            Reconstruction::Density {
                density: custom_density(),
                curve: DensityCurve::Sigmoid(SigmoidParams {
                    contrast: 1.7,
                    toe: 0.1,
                    shoulder: 0.4,
                    dmax: DmaxSource::Explicit(1.5),
                    // The golden vectors were captured with the anchor == dmax.
                    anchor: AnchorPlacement::WhiteAtDmax,
                }),
            },
            custom_print(),
            &[
                0xbbfb9f9b, 0xbc06d065, 0xbc09c550, 0x3bb51ff2, 0xb8997d80, 0xbafaba00, 0x3ef67b34,
                0x3ef5d7ac, 0x3ebb17ec, 0xbc0cc06c, 0x3f03d70a, 0x3f0a3d71, 0xbc019f5f, 0xbc09db81,
                0xbc0a489a,
            ],
            Some(0x3fc00000), // 1.5
            Some([0x3f800000, 0x3f866666, 0x3f8ccccd]),
            Some([0x3e4ccccd, 0x3fcccccd]),
        );
    }

    // --- the characteristic curve -------------------------------------------
    //
    // `algo/characteristic-curve-coverage`. Its tables are well covered in
    // `algo::film_stock::tests`, and the full chain's *properties* are pinned there too.
    // What follows is the bit-level half: captured numbers the code is measured against,
    // plus the argument for why they are portable.

    /// The reconstruction the two tests below measure: the default stock's published
    /// curve, with the identity gain this curve resolves for itself.
    ///
    /// The gain comes from its single definition rather than being restated — pairing
    /// `characteristic` with `DensityParams::default()`'s parametric calibration would
    /// correct the stock's own per-channel structure a second time, which is the trap
    /// `default_scale_for` exists to close.
    fn characteristic_config() -> Reconstruction {
        Reconstruction::Density {
            density: DensityParams {
                scale: DensityParams::default_scale_for(DensityCurveType::Characteristic),
                ..DensityParams::default()
            },
            curve: DensityCurve::Characteristic(CharacteristicParams::default()),
        }
    }

    /// The captured `reconstruct_and_print` result for [`characteristic_config`] over
    /// [`pixels`] / [`base`], as raw `f32` bits. All positive, which is what lets the
    /// golden below subtract bit patterns to measure a drift in ULPs.
    const CHARACTERISTIC_EXPECTED: [u32; 15] = [
        0x3c093270, 0x3c1d2b05, 0x3c120c73, // near-base shadow
        0x3dcb7eb3, 0x3da81c63, 0x3d9e0a6f, // midtone
        0x418c4e03, 0x4154240c, 0x409ddd62, // dense highlight (red is past the table)
        0x2cb14e9f, 0x4fd667b8, 0x4b7e2778, // out-of-range finite, extrapolated hard
        0x3b356c75, 0x3b356c75, 0x3b356c75, // exactly the base
    ];

    /// The characteristic curve's **wiring**, pinned to within 1 ULP.
    ///
    /// Every other golden here is bit-for-bit. This one is not, and the reason is
    /// measured rather than assumed: `characteristic_golden_values_carry_their_libm_headroom`
    /// shows that four of these fifteen values sit within 0.012 ULP of an f32 rounding
    /// boundary, where two libms may legitimately round `10f32.powf` in opposite
    /// directions. A bit-exact capture would be green on the host that took it and red on
    /// the other target — CLAUDE.md's cross-platform rule.
    ///
    /// The window is **uniform, not per-sample**, and that is deliberate: eleven of the
    /// fifteen are measured as safe, but only against the premise that both libms round a
    /// double result (documented for the `powf` glibc uses, unverified for Apple's).
    /// Tightening those eleven to bit-exact would make them hostage to that premise for
    /// no gain in detection.
    ///
    /// Nothing is lost as a regression pin. An edited table literal, a permuted channel
    /// and one table applied across all three each move these values by percent — the
    /// falsifiability matrix in `docs/progress/algo.md` (2026-09-10) records what each
    /// perturbation moved. The only difference a 1-ULP window hides is the one that is
    /// not portable anyway.
    ///
    /// One wiring fault it **cannot** see, because this config cannot: stages 1–2 are
    /// `scale·d + offset`, and at the identity gain and zero offset that this curve
    /// resolves for itself the transposed spelling is arithmetically the same. That one
    /// belongs to `algo::film_stock::tests::the_chain_applies_the_density_gain_before_the_offset`,
    /// which states it with an explicit non-neutral pair. The two halves of this task's
    /// coverage are complementary by design, not redundant.
    #[test]
    fn golden_characteristic_is_correct_within_one_ulp() {
        let (out, report) = reconstruct_and_print(
            &pixels(),
            &base(),
            &characteristic_config(),
            &PrintParams::default(),
        )
        .unwrap();
        // `zip` below truncates, so the length is asserted rather than assumed.
        assert_eq!(out.rgb.len(), CHARACTERISTIC_EXPECTED.len());
        for (i, (&want, &got)) in CHARACTERISTIC_EXPECTED
            .iter()
            .zip(out.rgb.iter())
            .enumerate()
        {
            let drift = ulps_between(got, f32::from_bits(want));
            assert!(
                drift <= 1,
                "sample {i}: {:08x} is {drift} ULP from the captured {want:08x}",
                got.to_bits()
            );
        }
        // This curve reads its reference and its placement off the film, so both are
        // absent — the property that makes it self-anchoring, asserted where a curve that
        // quietly acquired one would be caught.
        assert_eq!(report.dmax, None);
        assert_eq!(report.curve_anchor, None);
        assert_eq!(report.white_balance, Some([1.0, 1.0, 1.0]));
        assert_eq!(report.balance_range, None);
        // The extrapolation statistic is part of this render's output, and no other
        // golden carries one: the dense-highlight red and all three of the out-of-range
        // pixel's channels fall outside the published table.
        assert_eq!(
            report.out_of_table,
            Some(OutOfTable {
                below: [0.2, 0.0, 0.0],
                above: [0.2, 0.2, 0.2],
            })
        );
        assert_eq!(out.ir.as_deref(), Some(&[0.1f32, 0.2, 0.3, 0.4, 0.5][..]));
    }

    /// Rounding headroom, in f32 ULPs, below which two libms may legitimately disagree.
    ///
    /// **An empirical per-implementation bound, not a derived one** — and the derivation
    /// that looked like it justified this number was wrong, which is worth recording
    /// because it was convincing. It argued that computing in `f64` and rounding bounds
    /// the disagreement at `2^-5` of an f32 ULP. Work it through: one f64 ULP *is* about
    /// `2^-29` of an f32 ULP, so a few of them is `~2^-27 ≈ 7.5e-9` — seven orders
    /// tighter, and under *that* bound not one sample here is thin. The old figure came
    /// of converting the same quantity to a relative error twice.
    ///
    /// What `0.03` actually is: an implementation whose worst-case error is `E` ULPs
    /// returns the correctly-rounded result whenever the true value is more than
    /// `E − 0.5` ULPs from a boundary. glibc documents `powf` at **0.52 ULP**, giving
    /// `0.02`; this rounds up for margin. Apple's libm publishes no bound at all, and
    /// that — not the arithmetic — is the real uncertainty.
    ///
    /// So read the thin sets below as a **worst case**: the samples that could disagree
    /// if a libm is as bad as the worst one documents. At least one of them does — see
    /// [`THIN_LOG_SAMPLES`] — so the set is not merely theoretical padding either.
    const LIBM_SAFE_MARGIN_ULPS: f64 = 0.03;

    /// Distance from an f32 rounding boundary, in ULPs: `0.5` when `value` sits dead
    /// centre of its interval, `0` when it sits exactly on the boundary with its
    /// neighbour. `exact` is the true real result, `value` its correctly-rounded f32.
    ///
    /// The ULP is taken on the side `exact` lies, because at a binade boundary the step
    /// below a value is **half** the step above — measuring against the wrong side would
    /// overstate the margin, which is the unsafe direction. No sample here is a power of
    /// two, so today that is latent; it is written correctly anyway because the next
    /// recapture is not obliged to keep it that way.
    ///
    /// Precondition: `value` is finite and non-negative. An exact result (`value` equal
    /// to `exact`, as at the film-base pixel where `log10(1) = 0`) reports the full `0.5`,
    /// which is right: there is nothing for a libm to disagree about.
    fn boundary_margin(exact: f64, value: f32) -> f64 {
        let bits = value.to_bits();
        let toward = if exact >= f64::from(value) {
            bits + 1
        } else {
            bits - 1
        };
        let ulp = (f64::from(f32::from_bits(toward)) - f64::from(value)).abs();
        0.5 - ((exact - f64::from(value)) / ulp).abs()
    }

    /// ULPs between two finite f32s of the same sign, as a bit-pattern distance.
    fn ulps_between(a: f32, b: f32) -> i64 {
        debug_assert!(
            a.is_finite() && b.is_finite() && a.is_sign_positive() == b.is_sign_positive(),
            "ULP distance is a bit-pattern distance, so it needs finite same-signed inputs"
        );
        (i64::from(a.to_bits()) - i64::from(b.to_bits())).abs()
    }

    /// How far the **rendered** value moves when the corrected density `d` is perturbed by
    /// one ULP either way — i.e. how much a stage-1 `log10` disagreement is amplified by
    /// the time it reaches the pixel.
    ///
    /// Measured, never modelled. The closed form
    /// `ln(10) · d · (1/γ_local) · (ulp(d)/ulp(out))` under-predicts by up to 1.9x,
    /// because `log_e` is itself an f32 and the realized step quantizes to 0, 1 or 2 of
    /// its ULPs. A bound that under-predicts by 2x is the wrong sign for a safety
    /// threshold.
    fn stage_one_amplification(table: &[(f32, f32)], d: f32, rendered: f32) -> i64 {
        let render = |d: f32| {
            let (log_e, _) = invert(table, d);
            i64::from(10f32.powf(log_e).to_bits())
        };
        let at = i64::from(rendered.to_bits());
        [d.next_up(), d.next_down()]
            .into_iter()
            .map(|p| (render(p) - at).abs())
            .max()
            .expect("two perturbations")
    }

    /// The `10^` samples that sit inside [`LIBM_SAFE_MARGIN_ULPS`], with the margin
    /// measured at capture — the reason the golden above states 1 ULP.
    ///
    /// Samples 12-14 are the film-base pixel, whose corrected density is exactly `0.0`, so
    /// its value is `10^(table[0].0)`: a property of the shipped table literal rather than
    /// of this vector, and one that will reach *every* base-density pixel of every frame
    /// if `algo/split-default-migration` makes this curve the default.
    const THIN_POW_SAMPLES: &[(usize, f64)] =
        &[(10, 0.0120), (12, 0.0059), (13, 0.0059), (14, 0.0059)];

    /// The `log10` samples that sit inside [`LIBM_SAFE_MARGIN_ULPS`].
    ///
    /// Only one — and it is **confirmed to actually disagree**, not merely close: glibc's
    /// `log10f` on x86_64 returns a `d` one ULP off the correctly-rounded value here,
    /// while Apple's does not. CI found it on the first push, on precisely the sample
    /// this measurement had flagged, which is as direct a validation of the method as the
    /// repo is going to get.
    ///
    /// It is also the sample whose amplification is **zero**, so that disagreement does
    /// not reach the pixel and the golden passes on both targets. That coincidence is the
    /// whole reason the 1-ULP window is enough, and the conjunction assertion below is
    /// what stops it being relied on silently.
    const THIN_LOG_SAMPLES: &[(usize, f64)] = &[(1, 0.0153)];

    /// **The portability argument, machine-checked across the whole chain.**
    ///
    /// The task this closes recorded that a bit-exact capture "is not available" because
    /// `10f32.powf` differs ~1 ULP across libm implementations. That is true of the
    /// *values*, not of the mechanism, and which values is decidable in advance. `f64`
    /// resolves an f32 ULP to about 4e-9 of one, so every margin below is measured with
    /// seven orders of headroom over the threshold it is compared against.
    ///
    /// The chain makes **two** libm calls per sample, and both are measured here — an
    /// earlier version measured only the second and read as if it covered the chain:
    ///
    /// 1. `log10` in `algo::density::to_density`, producing the corrected density `d`;
    /// 2. `10f32.powf` in `algo::film_stock::apply_curve`, producing the pixel.
    ///
    /// A thin margin at (2) moves the pixel by 1 ULP, which the golden's window absorbs.
    /// A thin margin at (1) moves it by however much the curve **amplifies** it — up to
    /// 62 ULPs on this vector — which the window does not. So the golden is portable
    /// only while no single sample is thin at (1) *and* amplifying, and that conjunction
    /// is what this asserts. It is empty today by coincidence, not by construction:
    /// sample 1 is the one thin `log10` and the one sample the curve does not amplify.
    ///
    /// **That is not hypothetical.** x86_64 CI disagrees with this host on exactly
    /// sample 1's `log10`, and the golden passes there anyway because the curve flattens
    /// it. The conjunction is doing real work, not describing a risk that never fires.
    ///
    /// **The binding constraint on the whole change is sample 8** — `log10` margin
    /// 0.0353 against an amplification of 6 pixel ULPs. It clears glibc's documented
    /// `powf` bound (0.02) by 1.8x and this constant's padded 0.03 by only 1.1x. If
    /// Apple's `log10` turns out worse than either, this test stays green and
    /// `golden_characteristic_is_correct_within_one_ulp` reds on x86_64 by six times its
    /// window. That is the failure to expect, and its remedy is a different vector, not a
    /// wider tolerance.
    #[test]
    fn characteristic_golden_values_carry_their_libm_headroom() {
        let Reconstruction::Density { density, curve } = characteristic_config() else {
            unreachable!("the characteristic config is a density reconstruction")
        };
        let DensityCurve::Characteristic(params) = curve else {
            unreachable!("the characteristic config selects the characteristic curve")
        };
        // Read off the config rather than restated, so the two cannot name different stocks.
        let stock = curves_for(params.stock);
        let scan = pixels();
        let film_base = <[f32; 3]>::from(base());
        let densities = crate::algo::density::to_density(&scan, &base(), &density);
        assert_eq!(densities.density.len(), CHARACTERISTIC_EXPECTED.len());

        let mut thin_pow: Vec<(usize, f64)> = Vec::new();
        let mut thin_log: Vec<(usize, f64)> = Vec::new();
        let mut amplification = [0i64; 15];

        for (i, &d) in densities.density.iter().enumerate() {
            let c = i % 3;
            let table = stock.channels[c];
            let captured = f32::from_bits(CHARACTERISTIC_EXPECTED[i]);
            // `boundary_margin` and the golden's `i64` bit subtraction both assume a
            // positive finite f32; asserted rather than left to the prose, since a
            // recapture is what would break it.
            assert!(
                captured.is_finite() && captured > 0.0,
                "sample {i}: {captured} is outside what these measurements assume"
            );

            // --- stage 1: the `log10` that produced `d` ----------------------------
            // Stage 1 written out as the code writes it, not as a simplification —
            // two rounding details bite anyone who shortens it.
            //
            // **The ratio is divided in f32 first, and the reference has to be too.**
            // `to_density` takes `log10` of the *rounded* f32 quotient; dividing in f64
            // instead puts the reference 5 ULPs out and the margin becomes meaningless.
            // The division is plain IEEE and identical on both targets — only the
            // `log10` is libm.
            //
            // **And the gain/offset cannot be dropped even at identity.** The film-base
            // pixel's ratio is exactly 1, so the negated log is `-0.0`; it is the
            // `+ offset` that normalises the sign to `+0.0`, which is what `to_density`
            // stores.
            let ratio = scan.rgb[i].max(crate::algo::density::SCAN_EPSILON) / film_base[c];
            let exact_d = f64::from(density.scale[c]) * -f64::from(ratio).log10()
                + f64::from(density.offset[c]);
            let rounded_d = exact_d as f32;

            // **Conformance, not equality.** Demanding `d == rounded_d` would assert the
            // host libm is correctly rounding, which is precisely the thing that varies —
            // and it failed on x86_64 for sample 1, the very sample measured as thin. A
            // conforming libm lands within 1 ULP; that is the claim worth making.
            assert!(
                ulps_between(d, rounded_d) <= 1,
                "sample {i}: to_density's {d:e} is more than 1 ULP from the correctly                  rounded {rounded_d:e}"
            );
            let log_margin = boundary_margin(exact_d, rounded_d);
            if log_margin < LIBM_SAFE_MARGIN_ULPS {
                thin_log.push((i, log_margin));
            }

            // --- stage 3: the `10^` this vector was captured through ---------------
            // Fed the **correctly rounded** density, not the host's, so every number
            // below is a property of the values rather than of the machine measuring
            // them. Whether the host's own `d` reaches the same pixel is the golden's
            // question, and the conjunction at the end is what guarantees it.
            let (log_e, _) = invert(table, rounded_d);
            let exact_pow = 10f64.powf(f64::from(log_e));
            assert_eq!(
                (exact_pow as f32).to_bits(),
                CHARACTERISTIC_EXPECTED[i],
                "sample {i}: the captured value is not the correctly-rounded 10^{log_e}"
            );
            let pow_margin = boundary_margin(exact_pow, captured);
            if pow_margin < LIBM_SAFE_MARGIN_ULPS {
                thin_pow.push((i, pow_margin));
            }

            amplification[i] = stage_one_amplification(table, rounded_d, captured);
        }

        let recorded = |set: &[(usize, f64)]| set.iter().map(|(i, _)| *i).collect::<Vec<_>>();
        let measured = |set: &[(usize, f64)]| set.iter().map(|(i, _)| *i).collect::<Vec<_>>();
        assert_eq!(
            measured(&thin_pow),
            recorded(THIN_POW_SAMPLES),
            "the set of samples without `10^` headroom moved: measured {thin_pow:?}"
        );
        assert_eq!(
            measured(&thin_log),
            recorded(THIN_LOG_SAMPLES),
            "the set of samples without `log10` headroom moved: measured {thin_log:?}"
        );
        for (measured, recorded) in thin_pow
            .iter()
            .zip(THIN_POW_SAMPLES)
            .chain(thin_log.iter().zip(THIN_LOG_SAMPLES))
        {
            assert!(
                (measured.1 - recorded.1).abs() < 5e-4,
                "sample {}: margin moved from the recorded {} to {}",
                measured.0,
                recorded.1,
                measured.1
            );
        }

        // **The conjunction, and the reason the golden's window is 1 ULP.** A sample that
        // is both thin at stage 1 and amplified past the window would red on the other
        // target with nothing here to explain why.
        for &(i, margin) in &thin_log {
            assert!(
                amplification[i] <= 1,
                "sample {i} has a thin `log10` margin ({margin}) AND amplifies a stage-1 \
                 ULP to {} pixel ULPs. The golden's 1-ULP window can no longer carry this \
                 vector: pick sample values that clear the threshold, or widen the window \
                 deliberately and say why",
                amplification[i]
            );
        }
    }

    #[test]
    fn golden_simple_inversion_is_bit_identical() {
        // The pre-split simple converter with its (identity) default WB/clip —
        // the pure `1 − scan/Dmin` inversion must reproduce it exactly.
        assert_golden(
            Reconstruction::Simple,
            PrintParams::default(),
            &[
                0x3d638e30, 0x3dba2e90, 0x3dc30c30, 0x3f2aaaaa, 0x3f2c37da, 0x3f36db6e, 0x3f7a4fa5,
                0x3f7a6a20, 0x3f7a83a8, 0xbf2aaaac, 0x3fae8ba3, 0x3f800000, 0x00000000, 0x00000000,
                0x00000000,
            ],
            None,
            None,
            None,
        );
    }

    #[test]
    fn golden_auto_wb_estimation_is_bit_identical() {
        // The auto-WB path moved from "tone a strided density sample" to "stride
        // the film positive" — bit-identical because a per-sample map commutes
        // with striding. Pin both estimators' gains AND output pixels.
        assert_golden(
            frozen_reference_config(),
            PrintParams {
                white_balance: WbSource::Percentile,
                ..PrintParams::default()
            },
            &[
                0x43016967, 0x3c343958, 0x3c6d2311, 0x43b75553, 0x3cfa4fa3, 0x3d3bbbc3, 0x45abdfff,
                0x3eeaaaab, 0x3f1c71cd, 0x4292aaaa, 0x45abdfff, 0x45abdfff, 0x42f471c3, 0x3c23d70a,
                0x3c568d6f,
            ],
            Some(0x40000000),
            Some([1178532065, 1065353216, 1067949695]),
            None,
        );
        assert_golden(
            frozen_reference_config(),
            PrintParams {
                white_balance: WbSource::GrayWorld,
                ..PrintParams::default()
            },
            &[
                0x42e5eed9, 0x3c343958, 0x3c6d2125, 0x43a2de85, 0x3cfa4fa3, 0x3d3bba3d, 0x4598b09e,
                0x3eeaaaab, 0x3f1c7088, 0x42824b9f, 0x45abdfff, 0x45abde9a, 0x42d928b2, 0x3c23d70a,
                0x3c568bb2,
            ],
            Some(0x40000000),
            Some([1177135051, 1065353216, 1067949347]),
            None,
        );
    }

    #[test]
    fn golden_auto_wb_with_regional_balance_is_bit_identical() {
        // Auto-WB estimated on the POST-regional-balance positive (the ordering
        // contract) — the two features composed, with the exponential default
        // curve. Bits captured from the proven-bit-identical pipeline.
        assert_golden(
            Reconstruction::Density {
                density: balanced_density(),
                curve: frozen_reference_curve(),
            },
            PrintParams {
                white_balance: WbSource::Percentile,
                ..PrintParams::default()
            },
            &[
                0x4326b6f7, 0x3c343958, 0x3c67bd38, 0x43e5c3a8, 0x3cfb0056, 0x3d38792c, 0x45afe0e9,
                0x3ef021fc, 0x3f2016b4, 0x42961542, 0x45afe0e9, 0x45afe0e9, 0x431d73e7, 0x3c23d70a,
                0x3c51ab35,
            ],
            Some(0x40000000),
            Some([1180386296, 1065353216, 1068205576]),
            Some([0x3e4ccccd, 0x3fcccccd]), // the explicit [0.2, 1.6] echoed
        );
    }

    #[test]
    fn golden_auto_wb_with_sigmoid_curve_is_bit_identical() {
        // Auto-WB under the sigmoid curve: the estimator samples the S-curve's
        // film positive (not the exponential one), and the gains apply through
        // the same stage-4 slot.
        assert_golden(
            sigmoid_at_reference_anchor_2_0(),
            PrintParams {
                white_balance: WbSource::Percentile,
                ..PrintParams::default()
            },
            &[
                0x3b02e55f, 0x3b02af7d, 0x3b03a290, 0x3c811f4b, 0x3c7da965, 0x3ca7e975, 0x3f800000,
                0x3f72e4fa, 0x3f73a232, 0x3ad531a7, 0x3f800000, 0x3f800000, 0x3af4a59d, 0x3ae75cf6,
                0x3ae75cf6,
            ],
            Some(0x40000000),
            // RECAPTURED 2026-08-03: the auto-WB gain drops 2.2304 → 1.0574 because the
            // estimator samples the *rendered* positive, and the new sigmoid defaults produce
            // a far better-balanced one — so it has much less to correct. A large WB gain was
            // partly compensating for the old curve, which is worth knowing: WB and the curve
            // were entangled, and they are less so now.
            Some([0x3f875963, 0x3f800000, 0x3f800000]),
            None,
        );
    }

    #[test]
    fn golden_auto_measured_balance_range_is_bit_identical() {
        // The default `BalanceRange::Auto` with non-zero balances: the ramp
        // anchors are MEASURED from this frame's tone distribution (the other
        // regional-balance goldens use an explicit range), and both the measured
        // `[lo, hi]` and the resulting pixels are pinned.
        assert_golden(
            Reconstruction::Density {
                density: DensityParams {
                    balance_range: BalanceRange::Auto,
                    ..balanced_density()
                },
                curve: frozen_reference_curve(),
            },
            PrintParams::default(),
            &[
                0x3c42a1d5, 0x3c3439a6, 0x3c2cf03a, 0x3d084c85, 0x3cfa994a, 0x3d093901, 0x3eea9e5a,
                0x3eecf423, 0x3ee8a619, 0x3baf3a23, 0x45afe0e9, 0x45833ffb, 0x3c37d4dc, 0x3c23d70a,
                0x3c1c774b,
            ],
            Some(0x40000000),
            Some(UNIT_WB),
            Some([0, 1080930529]), // the frame-measured [lo, hi], captured verbatim
        );
    }

    // --- legacy no-preset regression ----------------------------------------
    //
    // No full-frame / whole-TIFF bit-exact hash is checked in: it can't be a
    // portable CI gate. The reconstruction's transcendental math (`10^`, `log10`)
    // diverges by ~1 ULP across libm implementations over a full frame — x86_64 CI
    // produced a different frame hash than the capture host, while the 5-pixel
    // per-pixel goldens matched exactly — and the downstream lcms2 color transform
    // + encode add further per-target bytes. nc's determinism contract is
    // byte-identity per build/architecture (design-spec §8), not across hosts.
    //
    // The per-pixel goldens above (a curated tonal-range + out-of-range vector with
    // dmax/white-balance/balance-range/IR all pinned, captured from the pre-split
    // code) are the portable bit-identity / no-`pipeline_version`-bump gate.
}
