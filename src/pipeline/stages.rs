//! Stage wiring as pure functions — threads film-base → reconstruction → the
//! selected output branch together for the orchestrator to call.
//!
//! This is the in-memory core of the `convert` pipeline (design-spec §6, stages
//! 3–5b). [`render_film_master`] owns the master bypass;
//! [`render_display_source`] is the shared display source every display preset
//! renders from, whose SDR/HDR/gain-map rendering and packaging the CLI
//! orchestrates afterward. Film-base estimation (stage 2) is the orchestrator's,
//! and decode (stage 1) and encode (the final stage) are I/O and stay with the
//! orchestrator (`cli`); everything here is pure `(input, params) -> output` so it
//! composes and unit-tests without touching the filesystem — with one documented
//! exception: the renders read a wall clock to fill [`StageTimings`] for the
//! telemetry record (a report-only channel; the pixels stay deterministic and
//! untouched by the measurement).
//!
//! The resolved [`OutputPreset`](crate::types::OutputPreset) routes through these
//! stage entrypoints:
//!
//! - **`film-master`** — `reconstruct → map_nc_film_rgb_v1 → render_split::film_master`:
//!   the mapped unclamped linear ACEScg buffer is encoded directly with the
//!   ACEScg ICC attached and **no** colour transform, print control, or display
//!   rendering.
//! - **every display preset** — `render_display_source` reconstructs once, maps to
//!   ACEScg, and applies the shared print controls once; the orchestrator then
//!   feeds that source to whichever display renderer(s) the preset needs.
//!
//! The `legacy` / `custom` branch — the print controls on film RGB before an ICC
//! transform — retired in `nf-retire/legacy-custom`; it lives on only in the
//! reference build. `golden` (below, `#[cfg(test)]`) pins the reconstruction every
//! branch shares.

use std::time::Instant;

use crate::algo;
use crate::pipeline::color::{self, OutputSpace};
use crate::pipeline::display_tone::Headroom;
use crate::pipeline::{render_split, sdr, working_space};
use crate::types::{FilmBase, LinearImage, PrintParams, Reconstruction, Result};

/// The in-memory pipeline result the orchestrator hands to the encoder: the
/// output image and the ICC blob to embed alongside it.
pub struct Rendered {
    pub image: LinearImage,
    pub icc: Vec<u8>,
    /// Resolved-value diagnostics (e.g. the anchor the curve used) for
    /// the JSON report.
    pub convert: ConvertReport,
    /// Wall-clock per-stage timings measured around the render's stages, for
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
/// reconstruction stage's resolved values plus the shared print controls'
/// resolved gains. A reporting channel, not a control surface (controls live in
/// the recipe structs).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConvertReport {
    /// The **derived** anchor the curve used — the corrected density that rendered to
    /// `1.0`, hence the black floor at `10^(−contrast·curve_anchor)`. `None` for the
    /// characteristic curve.
    pub curve_anchor: Option<f32>,
    /// The resolved white-balance gains `[r, g, b]` the shared print controls
    /// applied — the explicit gains, or the auto-estimated ones
    /// (`print.white_balance = gray-world | percentile`). Reported so a roll can
    /// freeze one frame's estimate into a recipe / `--white-balance` (measure
    /// once, reuse). `None` for `film-master`, which runs no print controls.
    pub white_balance: Option<[f32; 3]>,
    /// How far the frame fell outside the stock's published curve, per channel — `Some`
    /// only for the characteristic curve. Carried out so the orchestrator can warn: an
    /// out-of-table sample is extrapolated, not measured, and a frame with many of them is
    /// being rendered off the published data.
    pub out_of_table: Option<crate::algo::characteristic::OutOfTable>,
    /// The resolved regional-balance tone-ramp range `[lo, hi]` (corrected
    /// density), when the density reconstruction applied a shadow/highlight
    /// balance. `None` when both balances are the neutral
    /// `[0, 0, 0]`. Reported so a roll can reuse one frame's measured range via
    /// `--balance-range` (design-spec §9).
    pub balance_range: Option<[f32; 2]>,
}

/// The `display-p3` / `compatibility` render: the shared display source, one SDR
/// rendition in the requested gamut, and the output-encoded pixels plus their ICC.
///
/// Returns the same [`Rendered`] the film-master branch does, which is deliberate —
/// an SDR preset's product *is* a rendered image and a profile, so it reuses the
/// TIFF encode path rather than introducing a container of its own.
pub fn render_sdr_preset(
    image: &LinearImage,
    film_base: &FilmBase,
    reconstruction: &Reconstruction,
    print: &PrintParams,
    tone: Headroom,
    gamut: sdr::SdrGamut,
) -> Result<Rendered> {
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

/// The `film-master` branch: reconstruct → NC film RGB v1 → encode directly.
///
/// The bypass is carried by the signature: no print or output parameters reach
/// it, so no print control can run (`cli::validate` separately refuses a
/// requested one, so it is never silently dropped). No colour transform runs
/// either — the pixels are *already* linear ACEScg, and transforming again would
/// double-apply the matrix — and nothing is clamped. The ICC blob is the ACEScg
/// profile the values are genuinely in, fetched without building a transform.
///
/// Film-base estimation (stage 2) is deliberately **not** done here — the
/// orchestrator resolves the base first (via `film_base::estimate`) so it can
/// surface the estimate's quality warnings before this fallible render runs. Any
/// failure (a degenerate film base, an unusable curve anchor) surfaces as an
/// [`NcError`](crate::types::NcError) with the right exit code, never a
/// silently-wrong image. The IR plane is carried through untouched.
///
/// [`ConvertReport::white_balance`] stays `None` here by construction: no
/// white-balance stage ran, and reporting resolved gains for a master that
/// applied none would be a false provenance claim. The reconstruction's own
/// resolved diagnostics (`curve_anchor`, `balance_range`) *are* reported — they are part
/// of what the master contains.
pub fn render_film_master(
    image: &LinearImage,
    film_base: &FilmBase,
    reconstruction: &Reconstruction,
) -> Result<Rendered> {
    let started = Instant::now();
    let (film, recon) = algo::reconstruct(image, film_base, reconstruction)?;
    let master = render_split::film_master(working_space::map_nc_film_rgb_v1(film));
    let algorithm_ms = ms_since(started);

    let started = Instant::now();
    // Profile only — no transform: the tag names the space the pixels are in.
    let icc = color::icc_profile(&OutputSpace::AcesCg)?;
    let color_ms = ms_since(started);

    Ok(Rendered {
        image: master,
        icc,
        convert: ConvertReport {
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

/// Wall-clock durations of the two in-memory stages a render runs, in
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
    use super::*;
    use crate::types::{DensityCurve, DensityParams};

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

    /// The characteristic curve on the generic profile — the second producer that has to
    /// reach the master through the same mapper.
    fn characteristic_default() -> Reconstruction {
        Reconstruction {
            density: DensityParams::default(),
            curve: DensityCurve::Characteristic(crate::types::CharacteristicParams::default()),
        }
    }

    #[test]
    fn film_master_render_bypasses_the_colour_transform_and_print_controls() {
        // The film-master branch must hand back the *mapped* ACEScg pixels: no
        // print render (its signature takes no print parameters), and no colour
        // transform (which would re-apply the Rec.709→ACEScg matrix on values that
        // already crossed it). Pin that by recomputing the expected buffer through
        // the mapper directly.
        let img = synthetic_negative(8, 8);
        let base = FilmBase::from([0.9, 0.55, 0.42]);
        let out = render_film_master(&img, &base, &density_default()).unwrap();

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
        assert!(out.convert.curve_anchor.is_some());
    }

    #[test]
    fn film_master_render_works_for_every_reconstruction_path() {
        // The split is producer-agnostic: the exponential and the characteristic curve
        // both reach the master through the same mapper.
        let img = synthetic_negative(8, 8);
        let base = FilmBase::from([0.9, 0.55, 0.42]);
        for reconstruction in [density_default(), characteristic_default()] {
            let out = render_film_master(&img, &base, &reconstruction).unwrap();
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
        match render_film_master(&img, &base, &density_default()) {
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
    use crate::film_stock;
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
        tone: Headroom,
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
    /// Printed as a table rather than asserted per row: the exponential's placement depends
    /// on its anchor, so pinning its exact value here would pin another task's defaults.
    #[test]
    fn reinhard_is_free_at_mid_grey_and_priced_either_side_of_it() {
        let stock = FilmStock::Portra400;
        let print = PrintParams::default();
        let reinhard = Headroom::new(4.0).unwrap();
        // Resolve the per-channel gain from the curve, exactly as the recipe and CLI paths
        // do. Constructing `DensityParams::default()` beside a characteristic curve would
        // apply the exponential's calibration on top of one that already carries each
        // stock's per-channel response — the double-correction `default_scale_for`
        // documents, and the reason a table built that way reads ~0.06 stop dark on those
        // rows for no reason connected to what it is measuring.
        let density = |curve: crate::types::DensityCurve| Reconstruction {
            density: DensityParams {
                scale: DensityParams::default_scale_for(curve.curve_type()),
                ..DensityParams::default()
            },
            curve,
        };
        let cases: [(&str, Reconstruction); 3] = [
            (
                "characteristic (portra-400)",
                density(crate::types::DensityCurve::Characteristic(
                    CharacteristicParams { stock },
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
            "\n{:30}{:>10}{:>12}{:>10}",
            "reconstruction", "no tone", "reinhard", "cost"
        );
        for (name, reconstruction) in cases {
            let none = delivered_by(&reconstruction, stock, Headroom::new(0.0).unwrap(), &print);
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
            println!("{name:30}{:>10}{:>12}{cost:>10}", fmt(none), fmt(rein));
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

    fn delivered(stock: FilmStock, tone: Headroom, print: &PrintParams) -> [f32; 3] {
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
        // else measures the offset instead. The default gain `[1, 0.84, 0.73]` moves this
        // patch off 0.18 (it is calibrated for the exponential, which has no per-channel
        // film model; the characteristic curve already has one). That effect is the subject of
        // `the_default_gain_shifts_per_channel_level_on_both_curves`, not of these.
        let reconstruction = Reconstruction {
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
    /// delivered, at every headroom.
    #[test]
    fn mid_grey_lands_at_eighteen_percent_at_every_headroom() {
        let stock = FilmStock::Portra400;
        let print = PrintParams::default();
        // Red only: see `delivered`. Tone is the question here.
        let tone_of = |t, p: &PrintParams| delivered(stock, t, p)[0];

        // Zero headroom, the identity: gamut mapping and the range check still run, but
        // nothing reshapes tone.
        let none = tone_of(Headroom::new(0.0).unwrap(), &print);
        assert!(
            (none - 0.18).abs() < 0.01,
            "reconstruction placed the datasheet's mid-grey at {none:.4}, not 0.18 — the \
             darkness would then be the curve's, not the operator's"
        );

        // Extended Reinhard: free at mid-grey, at every headroom. Swept rather than
        // spot-checked because the gain is a function of the white point, so "independent
        // of the headroom" is the actual claim.
        let reinhard = tone_of(Headroom::new(4.0).unwrap(), &print);
        let stops = (none / reinhard).log2();
        assert!(
            stops.abs() < 0.02,
            "extended Reinhard cost {stops:.3} stop at mid-grey, expected none"
        );
        for headroom in [0.0f32, 1.0, 4.0, 6.0, 10.0] {
            let at = tone_of(Headroom::new(headroom).unwrap(), &print);
            assert!(
                (at - 0.18).abs() < 0.005,
                "{headroom} stops of headroom delivered {at:.4}, not 0.18"
            );
        }

        println!(
            "mid-grey delivered (red) — identity {none:.4}, reinhard {reinhard:.4} \
             ({stops:.3} stop)"
        );
    }

    /// **Each `--preset` carries the exposure that lands the calibration target on the
    /// calibration stock.**
    ///
    /// What a preset promises is that *switching* it changes the look rather than the
    /// brightness, so a comparison is about the reconstruction and the display tone
    /// instead of "one is brighter". That is a **calibration convenience, not a rendering
    /// goal.** nc does not promise that two presets render the same picture, and does not
    /// promise a common mid-grey on every stock; making them agree is not what any of
    /// them exists for, and a preset that suits a film better by sitting slightly off is
    /// doing its job.
    ///
    /// So the assertion is scoped to the stock the constants were solved on
    /// (`Portra400`), and every other stock is **printed, never asserted**. A residual
    /// there measures how well that preset models that film: `characteristic-stock`
    /// inverts the very curve the patch is built from and so lands the same value on all
    /// of them, while the generic profile drifts with the stock. That drift is information
    /// about the reconstruction, not a constant to tune away — per-stock exposures would
    /// buy uniformity nobody asked for at the cost of more numbers to keep true.
    ///
    /// Synthetic and asset-free, on the same datasheet mid-grey patch as the rest of this
    /// module. `generic-c41` is skipped throughout — it is derived rather than measured,
    /// so it carries neither an aim table nor a `d_min` to build a patch from.
    ///
    /// The tolerance is 0.15 stop. The exposures are stated to two decimals, which is
    /// ±0.005 stop of quantization on its own; the residuals run to 0.06 because a solved
    /// exposure was rounded, not because a bundle drifted.
    #[test]
    fn presets_land_the_calibration_target_on_the_calibration_stock() {
        use crate::cli::ConversionPreset;
        // Scene mid-grey 0.18 rendered 1.33 stop up — the brightness approved
        // 2026-09-15, and the same target the exposure table above solves against.
        let target = 0.18 * 2f32.powf(1.33);
        // The stock every preset constant was solved on.
        let calibration = FilmStock::Portra400;

        // `None` when the preset refuses the stock (`characteristic-aim` has no usable
        // aim delta on three of them) or the renderer refuses the bundle.
        let delivered_red = |preset: ConversionPreset, stock: FilmStock| -> Option<f32> {
            let e = preset.expand(Some(stock)).ok()?;
            let print = PrintParams {
                print_exposure: e.print_exposure,
                ..PrintParams::default()
            };
            let tone = Headroom::default();
            let reconstruction = Reconstruction {
                density: DensityParams {
                    scale: e.density_scale,
                    ..DensityParams::default()
                },
                curve: e.curve,
            };
            delivered_by(&reconstruction, stock, tone, &print).map(|rgb| rgb[0])
        };

        println!(
            "\n  target {target:.4}\n\n  asserted — {}\n\n  {:24}{:>11}{:>12}",
            calibration.as_str(),
            "preset",
            "delivered",
            "stop"
        );
        let mut off = Vec::new();
        for preset in ConversionPreset::ALL {
            // A refusal on the calibration stock is a real failure, so it is never skipped.
            let red = delivered_red(preset, calibration).unwrap_or_else(|| {
                panic!("{} was refused on {}", preset.name(), calibration.as_str())
            });
            let stops = (red / target).log2();
            println!("  {:24}{:>11.4}{:>+12.3}", preset.name(), red, stops);
            if stops.abs() >= 0.15 {
                off.push(format!(
                    "{} delivered {red:.4} ({stops:+.3} stop)",
                    preset.name()
                ));
            }
        }
        // Collected, not asserted per row: recalibrating the family means reading every
        // preset's offset from one run, and a per-row assert hides the rest behind the
        // first one that misses.
        assert!(
            off.is_empty(),
            "{} of {} presets miss the calibration target {target:.4} on {}: {}",
            off.len(),
            ConversionPreset::ALL.len(),
            calibration.as_str(),
            off.join("; ")
        );

        // Printed and never asserted. One brightness across *stocks* is not a goal, so
        // this table is a description of the family rather than a bound on it: the
        // spread is what each preset's modelling of that film costs.
        println!("\n  printed only — stop from target, every measured stock\n");
        // Wide enough for the longest preset name, so the header sits over its column.
        const COL: usize = 23;
        print!("  {:16}", "stock");
        for preset in ConversionPreset::ALL {
            print!("{:>COL$}", preset.name());
        }
        println!();
        for &stock in FilmStock::ALL {
            // The derived generic states no aim table and no d_min, so there is no
            // datasheet patch to build from it.
            if film_stock::curves_for(stock).aims.is_none() {
                continue;
            }
            print!("  {:16}", stock.as_str());
            for preset in ConversionPreset::ALL {
                match delivered_red(preset, stock) {
                    Some(red) => print!("{:>+COL$.3}", (red / target).log2()),
                    None => print!("{:>COL$}", "refused"),
                }
            }
            println!();
        }
    }

    /// How much per-channel *level* does the default gain move, on each curve?
    ///
    /// Checked because an earlier write-up claimed "≤0.06 stop, the anchoring absorbs it",
    /// measured from whole-image means through `hanten convert`. That measurement was bad: two
    /// of the three frames sat near clipping, where the mean cannot move. On a mid-grey
    /// patch the shift is an order of magnitude larger, and it is a **level**, not the
    /// tilt the drift probes measure — so it is what actually decides whether the render
    /// reads magenta.
    #[test]
    fn the_default_gain_shifts_per_channel_level_on_both_curves() {
        let stock = FilmStock::Portra400;
        let print = PrintParams::default();
        let identity = DensityParams {
            scale: [1.0, 1.0, 1.0],
            ..DensityParams::default()
        };
        let cases: [(&str, crate::types::DensityCurve); 2] = [
            (
                "characteristic",
                crate::types::DensityCurve::Characteristic(CharacteristicParams { stock }),
            ),
            ("exponential", crate::types::DensityCurve::default()),
        ];
        println!(
            "\n  {:16}{:>10}{:>10}{:>10}   per-channel stops vs identity",
            "curve", "R", "G", "B"
        );
        let mut worst = 0.0f32;
        for (name, curve) in cases {
            let at = |density: DensityParams| {
                let reconstruction = Reconstruction { density, curve };
                delivered_by(&reconstruction, stock, Headroom::new(0.0).unwrap(), &print)
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
            ("exponential", crate::types::DensityCurve::default()),
        ] {
            let at = |density: DensityParams| {
                delivered_by(
                    &Reconstruction { density, curve },
                    stock,
                    Headroom::new(0.0).unwrap(),
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

/// Golden fixtures pinning the reconstruction ([`algo::reconstruct`]) bit-for-bit
/// against the **pre-split monolithic converters** (`Algorithm::{Simple,Density,Sigmoid}`
/// — the `simple` and sigmoid captures left with those paths in
/// `nf-retire/sigmoid-and-simple`, so the density ones remain).
/// Most expected values below were captured by running the pre-refactor code on
/// these exact inputs and printing `f32::to_bits` — so any arithmetic drift in the
/// split (a reordered multiply, a changed intermediate, a lost anchor) fails these
/// tests immediately, not in a downstream image diff. That was the split task's
/// acceptance gate: the split is a structural refactor, the default pixels are the
/// contract.
///
/// **Two of the seven are not reference captures**, and the claim has to be scoped or it
/// stops being true. `golden_new_default_is_bit_identical` is captured fresh from the
/// build each time the default moves (last on 2026-09-23, for `pipeline_version` 6), and
/// `golden_density_exponential_customized_is_bit_identical` was recaptured on 2026-09-23
/// without the retired print stage. (The characteristic golden is pinned by its own
/// correctly-rounded derivation, not by a capture.) Both honestly pin "this has not drifted
/// since it was set", which is strictly weaker than "matches the reference
/// implementation" — no golden can claim the stronger thing about a value that was
/// deliberately changed. Each says so at its own call site; read it before treating
/// a failure there as proof of a regression. Nothing here hashes an encoded TIFF:
/// see the note at the end of the module for why that could never be checked in.
///
/// **They pinned `reconstruct_and_print` until `nf-retire/legacy-custom`** retired the
/// legacy print stage. Under default print settings that stage was a bit-exact identity
/// (a `2^0` gain, unit gains, a zero black point and a disabled soft clip), so every
/// default-print capture carried over unchanged. The customized cases ran a non-default
/// print, and were recaptured without it on 2026-09-23 — so they now pin "has not
/// drifted since", like the default above. The auto-white-balance cases
/// pinned the legacy estimate on film RGB before the 3×3, a placement no chain has any
/// more, and went with it.
///
/// The module is (test-only) `pub(crate)` so the `pipeline_version` drift gate in
/// [`crate::version`] fingerprints **these exact** curated vectors instead of a
/// second copy that could quietly drift away from them.
#[cfg(test)]
pub(crate) mod golden {
    use super::*;
    use crate::algo::characteristic::{OutOfTable, invert};
    use crate::film_stock::curves_for;
    use crate::types::{
        AnchorPlacement, BalanceRange, CharacteristicParams, DensityCurve, DensityCurveType,
        DensityParams, ExponentialParams,
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
    /// — the block the auto-range golden was captured with (unlike
    /// [`custom_density`], scale/offset stay at their defaults).
    fn balanced_density() -> DensityParams {
        DensityParams {
            shadow_balance: [0.05, 0.0, -0.02],
            highlight_balance: [-0.05, 0.01, 0.0],
            balance_range: BalanceRange::Explicit([0.2, 1.6]),
            ..frozen_density()
        }
    }

    /// [`algo::reconstruct`] over [`pixels`] / [`base`], as a plain image plus its
    /// report. Shared with the drift gate, so both measure the same call.
    pub(crate) fn reconstructed(
        reconstruction: &Reconstruction,
    ) -> (LinearImage, algo::ReconstructionReport) {
        let (film, report) = algo::reconstruct(&pixels(), &base(), reconstruction)
            .expect("the reconstruction must succeed on the curated vectors");
        (film.into_linear(), report)
    }

    /// Assert the reconstructed pixels (and the resolved diagnostics) match the
    /// captured bits exactly.
    fn assert_golden(
        reconstruction: Reconstruction,
        expected_rgb_bits: &[u32],
        expected_anchor_bits: Option<u32>,
        expected_range_bits: Option<[u32; 2]>,
    ) {
        let (out, report) = reconstructed(&reconstruction);
        let got: Vec<u32> = out.rgb.iter().map(|v| v.to_bits()).collect();
        assert_eq!(got, expected_rgb_bits, "pixel bits drifted");
        assert_eq!(
            report.curve_anchor.map(f32::to_bits),
            expected_anchor_bits,
            "anchor"
        );
        assert_eq!(
            report.balance_range.map(|r| r.map(f32::to_bits)),
            expected_range_bits,
            "balance range"
        );
        // IR rides through untouched on every path.
        assert_eq!(out.ir.as_deref(), Some(&[0.1f32, 0.2, 0.3, 0.4, 0.5][..]));
    }

    /// The identity per-channel gain, for every capture that is **frozen** against a
    /// reference implementation or a past build rather than tracking the live default.
    ///
    /// `DensityParams::default()`'s gain became `[1, 0.90, 0.86]` in `pipeline_version` 4
    /// and `[1, 0.84, 0.73]` in 5 — a scanner calibration, and one that has now moved
    /// twice, which is the whole argument for this helper. A capture whose meaning is
    /// "this still agrees with the reference implementation's arithmetic" must not absorb
    /// that: recapturing it under a new default would keep the test green while silently
    /// deleting the agreement it exists to prove. Only
    /// `golden_new_default_is_bit_identical` tracks the default, and it is recaptured on
    /// each move.
    fn frozen_density() -> DensityParams {
        DensityParams {
            scale: [1.0, 1.0, 1.0],
            ..DensityParams::default()
        }
    }

    /// The mid-above-base offset whose anchor at `gamma` is **exactly** `anchor`.
    ///
    /// The reference captures below were taken with the anchor stated directly, under a
    /// placement that pinned white at a reference density (retired with it,
    /// `nf-retire/dmax-machinery`). The curve reads only the anchor, so reaching the same
    /// `f32` anchor through the one placement left renders the same bits — which is what
    /// keeps them captures of the reference arithmetic rather than re-descriptions of
    /// this build. Searched rather than solved, because `offset + 0.745/gamma` rounds; the
    /// assertion is what makes "exactly" a checked claim.
    fn offset_reaching(anchor: f32, gamma: f32) -> f32 {
        let mut offset = anchor - crate::types::MID_GREY_OUTPUT_DECADES / gamma;
        for _ in 0..8 {
            let got = AnchorPlacement::MidAtBaseOffset(offset).anchor(gamma);
            if got.to_bits() == anchor.to_bits() {
                return offset;
            }
            offset = if got < anchor {
                offset.next_up()
            } else {
                offset.next_down()
            };
        }
        panic!("no offset reaches anchor {anchor} at gamma {gamma} exactly")
    }

    /// The configuration the vectors below were captured under: the exponential
    /// straight line at gamma **1.0** with the anchor at **2.0** (the nominal
    /// reference density of the day, pinned at white).
    ///
    /// Pinned **explicitly** rather than through `DensityCurve::default()`, because
    /// the default has moved twice since (`pipeline_version` 2 and 6). A golden vector
    /// that silently follows the default stops pinning anything the moment the default
    /// moves — it just re-describes whatever the build now does. Naming the
    /// configuration keeps every bit below exactly as captured from the reference
    /// code, and the *new* default gets its own golden (`golden_new_default_...`).
    fn frozen_reference_curve() -> DensityCurve {
        DensityCurve::Exponential(ExponentialParams {
            gamma: 1.0,
            anchor: AnchorPlacement::MidAtBaseOffset(offset_reaching(2.0, 1.0)),
        })
    }

    /// `Reconstruction::default()`'s density knobs with [`frozen_reference_curve`].
    fn frozen_reference_config() -> Reconstruction {
        Reconstruction {
            density: frozen_density(),
            curve: frozen_reference_curve(),
        }
    }

    #[test]
    fn golden_new_default_is_bit_identical() {
        // THE default path as of `pipeline_version` 6 (2026-09-23): the exponential at the
        // fixed decode's configuration — contrast 2.0, mid-grey pinned 0.62 above the film
        // base, gain `[1, 0.84, 0.73]`.
        //
        // Captured from THIS build, not from the reference implementation — so it pins
        // "the default has not drifted since it was set", which is what a default golden
        // can honestly claim after the default deliberately moved. The reference-derived
        // captures live in the `frozen_reference_*` goldens above and say
        // `frozen_density()` explicitly, so a future default cannot rewrite what they
        // verify. The v2–v5 captures were the sigmoid's and left with it.
        assert_golden(
            Reconstruction::default(),
            &[
                0x3c3e41ab, 0x3c472c7f, 0x3c4467a7, 0x3dbeeaca, 0x3d8a8882, 0x3d841ca2, 0x41a7cc5e,
                0x40ccbdff, 0x403537ec, 0x3b745fb9, 0x4c2dff42, 0x49cd08c6, 0x3c29b443, 0x3c29b443,
                0x3c29b443,
            ],
            Some(0x3f7e0b8d), // 0.62 + 0.745/2
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
            &[
                0x3c2d7a46, 0x3c343958, 0x3c35161a, 0x3cf5c28f, 0x3cfa4fa3, 0x3d0f5c2a, 0x3ee66668,
                0x3eeaaaab, 0x3eeeeef1, 0x3bc49ba7, 0x45abdfff, 0x45833ffb, 0x3c23d70a, 0x3c23d70a,
                0x3c23d70a,
            ],
            Some(0x40000000), // the anchor these bits were captured at
            None,
        );
    }

    #[test]
    fn golden_density_exponential_customized_is_bit_identical() {
        // Every density knob non-default at once: scale/offset, regional balance
        // with an explicit range, gamma 1.4, and an explicit anchor.
        //
        // RECAPTURED 2026-09-23 (`nf-retire/legacy-custom`) without the custom print
        // it used to carry: that stage retired, and the reconstruction half it sat on
        // is unchanged — re-applying the retired print arithmetic to these bits
        // reproduces the previous capture to f32 rounding.
        assert_golden(
            Reconstruction {
                density: custom_density(),
                curve: DensityCurve::Exponential(ExponentialParams {
                    gamma: 1.4,
                    anchor: AnchorPlacement::MidAtBaseOffset(offset_reaching(1.8, 1.4)),
                }),
            },
            &[
                0x3b952b3e, 0x3b622ad1, 0x3b332991, 0x3cb28033, 0x3c6d3ed9, 0x3c40dcef, 0x3f87e170,
                0x3f2900bc, 0x3ea6cf04, 0x3ab43eb3, 0x48a5a519, 0x46f463df, 0x3b88998c, 0x3b45ea62,
                0x3b1def80,
            ],
            Some(0x3fe66666),               // 1.8
            Some([0x3e4ccccd, 0x3fcccccd]), // [0.2, 1.6]
        );
    }

    // --- the characteristic curve -------------------------------------------
    //
    // `algo/characteristic-curve-coverage`. Its tables are covered in `film_stock::tests`,
    // and the full chain's *properties* are pinned in `algo::characteristic::tests`.
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
        Reconstruction {
            density: DensityParams {
                scale: DensityParams::default_scale_for(DensityCurveType::Characteristic),
                ..DensityParams::default()
            },
            curve: DensityCurve::Characteristic(CharacteristicParams::default()),
        }
    }

    /// The captured reconstruction of [`characteristic_config`] over
    /// [`pixels`] / [`base`], as raw `f32` bits. All positive, which is what lets the
    /// golden below subtract bit patterns to measure a drift in ULPs.
    const CHARACTERISTIC_EXPECTED: [u32; 15] = [
        0x3c093270, 0x3c1d2b05, 0x3c120c73, // near-base shadow
        0x3dcb7eb3, 0x3da81c63, 0x3d9e0a6f, // midtone
        0x418c4e03, 0x4154240c, 0x409ddd62, // dense highlight (red is past the table)
        0x2cb14e9f, 0x4fd667b8, 0x4b7e2778, // out-of-range finite, extrapolated hard
        0x3b356c75, 0x3b356c75, 0x3b356c75, // exactly the base
    ];

    /// ULPs between two finite f32s of the same sign, as a bit-pattern distance.
    fn ulps_between(a: f32, b: f32) -> i64 {
        debug_assert!(
            a.is_finite() && b.is_finite() && a.is_sign_positive() == b.is_sign_positive(),
            "ULP distance is a bit-pattern distance, so it needs finite same-signed inputs"
        );
        (i64::from(a.to_bits()) - i64::from(b.to_bits())).abs()
    }

    /// The accuracy premise every measurement below rests on: a libm worth shipping is
    /// within 1 ULP on these functions.
    ///
    /// Deliberately the **weak** bound. Both shipped targets are far better, and an
    /// earlier version of this harness tried to exploit that — classifying a sample
    /// "safe" when its true value sat further from an f32 rounding boundary than glibc's
    /// documented `powf` excess (~0.02 ULP). Observation killed it. On x86_64 glibc's
    /// `log10f` returns a different `f32` from Apple's for **sample 9**, whose margin is
    /// 0.456 ULP — twenty times the threshold that had called it safe — and for sample 1
    /// at 0.0153. A published bound for one function does not transfer to another, and
    /// neither target documents `log10f` at all.
    ///
    /// So nothing here infers agreement from a margin. The window is derived by
    /// enumerating what a conforming libm can actually return.
    const LIBM_MAX_ERROR_ULPS: i64 = 1;

    /// Sanity ceiling on a derived window. Not a tuning knob — it exists so that a future
    /// vector landing on a near-vertical stretch of a published curve (`PORTRA_160_B` has
    /// a segment with `1/γ = 767`) is reported rather than silently granted an enormous
    /// tolerance.
    const MAX_REASONABLE_WINDOW_ULPS: i64 = 200;

    /// Every pixel value a conforming libm can produce for one sample, as a window in
    /// ULPs around the captured value.
    ///
    /// **Sound by enumeration, not by argument.** `to_density`'s `log10` may return any
    /// f32 within [`LIBM_MAX_ERROR_ULPS`] of the correctly-rounded density, so each is
    /// rendered and the widest excursion taken; the curve's own `10^` may then be off by
    /// the same again, which is the final term. Any target meeting the premise lands
    /// inside this, whatever its individual error bounds are and whether or not anyone
    /// publishes them.
    ///
    /// The window is wide where the curve is steep: a 1-ULP density difference is
    /// amplified by `ln(10)·d·(1/γ_local)`, reaching 62 ULPs on the out-of-range pixel.
    /// That costs nothing in detection — a real fault moves these pixels by ~10^5 ULPs
    /// (measured: a `1e-6` nudge to one table literal moves them 115,523).
    ///
    /// **It measures the reachable set's own spread, and deliberately never looks at the
    /// captured value.** A first version measured each render's distance *from the
    /// capture*, which let a table edit inflate the window by exactly as much as it
    /// inflated the drift — the golden then passed on a frame whose every pixel had
    /// moved 115,523 ULPs. A window that depends on the value under test is not a window.
    /// The falsifiability run is what caught it; nothing else would have.
    fn reachable_window(table: &[(f32, f32)], rounded_d: f32) -> i64 {
        let render = |d: f32| {
            let (log_e, _) = invert(table, d);
            10f32.powf(log_e)
        };
        let centre = render(rounded_d);
        let widest = [rounded_d.next_down(), rounded_d.next_up()]
            .into_iter()
            .map(|d| ulps_between(render(d), centre))
            .max()
            .expect("two neighbouring densities");
        widest + LIBM_MAX_ERROR_ULPS
    }

    /// The **correctly rounded** corrected density of each sample, computed in f64.
    ///
    /// The centre of each sample's window, and target-independent by construction —
    /// which is the point: the host's own `to_density` output is one of the values a
    /// conforming libm may return, not the reference.
    ///
    /// Stage 1 is written out as the code writes it, and two rounding details bite anyone
    /// who shortens it. **The ratio is divided in f32 first**: `to_density` takes `log10`
    /// of the *rounded* f32 quotient, and dividing in f64 instead puts the reference 5
    /// ULPs out. **And the gain/offset cannot be dropped even at identity** — the
    /// film-base pixel's ratio is exactly 1, so the negated log is `-0.0`, and it is the
    /// `+ offset` that normalises the sign to the `+0.0` actually stored.
    fn correctly_rounded_densities(density: &DensityParams) -> Vec<f32> {
        let scan = pixels();
        let film_base = <[f32; 3]>::from(base());
        scan.rgb
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let c = i % 3;
                let ratio = s.max(crate::algo::density::SCAN_EPSILON) / film_base[c];
                let exact = f64::from(density.scale[c]) * -f64::from(ratio).log10()
                    + f64::from(density.offset[c]);
                exact as f32
            })
            .collect()
    }

    /// The characteristic curve's **wiring**, pinned within a derived per-sample window.
    ///
    /// Every other golden here is bit-for-bit. This one cannot be, and the reason is
    /// measured rather than assumed: the chain evaluates two libm functions per sample —
    /// `log10` in `to_density` and `10^` in the curve — and the two shipped targets
    /// **observably disagree** on the first. x86_64's `log10f` returns a different `f32`
    /// from Apple's on two of these fifteen samples. A bit-exact capture would be green
    /// on the host that took it and red on the other, which is CLAUDE.md's cross-platform
    /// rule.
    ///
    /// The window is not a chosen tolerance. [`reachable_window`] enumerates every pixel
    /// a libm meeting [`LIBM_MAX_ERROR_ULPS`] can produce for each sample and takes the
    /// widest, so this passes on any conforming target by construction rather than by the
    /// luck of which way a rounding fell. (It was luck, once: the x86_64 disagreement on
    /// sample 9 happens to fall in the direction the curve flattens, so an earlier
    /// 1-ULP window passed CI while resting on nothing.)
    ///
    /// Nothing is lost as a regression pin. An edited table literal, a permuted channel
    /// and one table applied across all three each move these values by percent — the
    /// falsifiability matrix in `docs/progress/algo.md` (2026-09-10) records what each
    /// perturbation moved, the smallest being ~10^5 ULPs against a worst-case window of
    /// 63.
    ///
    /// One wiring fault it **cannot** see, because this config cannot: stages 1–2 are
    /// `scale·d + offset`, and at the identity gain and zero offset that this curve
    /// resolves for itself the transposed spelling is arithmetically the same. That one
    /// belongs to `algo::characteristic::tests::the_chain_applies_the_density_gain_before_the_offset`,
    /// which states it with an explicit non-neutral pair. The two halves of this task's
    /// coverage are complementary by design, not redundant.
    #[test]
    fn golden_characteristic_is_correct_within_its_libm_window() {
        let Reconstruction { density, curve } = characteristic_config();
        let DensityCurve::Characteristic(params) = curve else {
            unreachable!("the characteristic config selects the characteristic curve")
        };
        let stock = curves_for(params.stock);
        let rounded = correctly_rounded_densities(&density);

        let (out, report) = reconstructed(&characteristic_config());
        // `zip` below truncates, so the length is asserted rather than assumed.
        assert_eq!(out.rgb.len(), CHARACTERISTIC_EXPECTED.len());

        for (i, (&want, &got)) in CHARACTERISTIC_EXPECTED
            .iter()
            .zip(out.rgb.iter())
            .enumerate()
        {
            let captured = f32::from_bits(want);
            let window = reachable_window(stock.channels[i % 3], rounded[i]);
            let drift = ulps_between(got, captured);
            assert!(
                drift <= window,
                "sample {i}: {:08x} is {drift} ULP from the captured {want:08x}, outside \
                 the {window} ULP any conforming libm can reach",
                got.to_bits()
            );
        }

        // This curve reads its placement off the film, so no anchor is reported — the
        // property that makes it self-anchoring, asserted where a curve that quietly
        // acquired one would be caught.
        assert_eq!(report.curve_anchor, None);
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

    /// The capture is intact and the host is conforming — the two things the golden's
    /// derived window assumes but cannot check for itself.
    ///
    /// It fails if a table literal moves, if the vector changes, if a captured constant
    /// is edited to something no correctly-rounded evaluation produces, or if a host's
    /// libm is worse than [`LIBM_MAX_ERROR_ULPS`] — which would invalidate every window
    /// the golden derives.
    #[test]
    fn the_characteristic_capture_is_correctly_rounded_and_the_host_conforms() {
        let Reconstruction { density, curve } = characteristic_config();
        let DensityCurve::Characteristic(params) = curve else {
            unreachable!("the characteristic config selects the characteristic curve")
        };
        let stock = curves_for(params.stock);
        let rounded = correctly_rounded_densities(&density);
        let host = crate::algo::density::to_density(&pixels(), &base(), &density);
        assert_eq!(host.density.len(), CHARACTERISTIC_EXPECTED.len());

        let mut widest = 0;
        for (i, (&want, &host_d)) in CHARACTERISTIC_EXPECTED
            .iter()
            .zip(host.density.iter())
            .enumerate()
        {
            let captured = f32::from_bits(want);
            assert!(
                captured.is_finite() && captured > 0.0,
                "sample {i}: {captured} is outside what `ulps_between` assumes"
            );

            // **Conformance, not equality.** Requiring the host's `log10` to equal the
            // correctly-rounded value asserts the host rounds correctly, which is exactly
            // what varies — it red x86_64 on sample 1.
            let off = ulps_between(host_d, rounded[i]);
            assert!(
                off <= LIBM_MAX_ERROR_ULPS,
                "sample {i}: this host's `log10` is {off} ULP from the correctly rounded \
                 {:e}, beyond the accuracy every derived window assumes",
                rounded[i]
            );

            // Capture integrity: the constant is what a correctly-rounded chain produces.
            // `f64` resolves an f32 ULP to ~4e-9 of one, so this is not a close call.
            let (log_e, _) = invert(stock.channels[i % 3], rounded[i]);
            assert_eq!(
                (10f64.powf(f64::from(log_e)) as f32).to_bits(),
                want,
                "sample {i}: the captured value is not the correctly-rounded 10^{log_e}"
            );

            widest = widest.max(reachable_window(stock.channels[i % 3], rounded[i]));
        }

        assert!(
            widest <= MAX_REASONABLE_WINDOW_ULPS,
            "the widest derived window is now {widest} ULP — a sample has landed somewhere \
             the curve amplifies steeply, and the golden's tolerance should be understood \
             before it is accepted"
        );
    }

    #[test]
    fn golden_auto_measured_balance_range_is_bit_identical() {
        // The default `BalanceRange::Auto` with non-zero balances: the ramp
        // anchors are MEASURED from this frame's tone distribution (the other
        // regional-balance goldens use an explicit range), and both the measured
        // `[lo, hi]` and the resulting pixels are pinned.
        assert_golden(
            Reconstruction {
                density: DensityParams {
                    balance_range: BalanceRange::Auto,
                    ..balanced_density()
                },
                curve: frozen_reference_curve(),
            },
            &[
                0x3c42a1d5, 0x3c3439a6, 0x3c2cf03a, 0x3d084c85, 0x3cfa994a, 0x3d093901, 0x3eea9e5a,
                0x3eecf423, 0x3ee8a619, 0x3baf3a23, 0x45afe0e9, 0x45833ffb, 0x3c37d4dc, 0x3c23d70a,
                0x3c1c774b,
            ],
            Some(0x40000000),
            Some([0, 1080930529]), // the frame-measured [lo, hi], captured verbatim
        );
    }

    // --- why no whole-frame hash ---------------------------------------------
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
    // anchor/balance-range/IR all pinned, captured from the pre-split code) are the
    // portable bit-identity / no-`pipeline_version`-bump gate. They stop at
    // `algo::reconstruct`: nothing committed guards display stages or post-lcms2
    // output across targets, so a change there is verified by same-machine
    // before/after comparison, never by a checked-in vector.
}
