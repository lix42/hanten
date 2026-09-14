//! Test-only probe for `algo/film-stock-profiles`: does inverting a stock's published
//! characteristic curve remove the exposure-dependent channel cast that a single scalar
//! contrast leaves behind?
//!
//! **The question.** The datasheets are the film's response to a *neutral* step wedge, and
//! they say the blue layer runs 12–19 % steeper than red on every C-41 stock measured. nc
//! today applies one scalar contrast to all three channels, which asserts the opposite. If
//! the datasheets describe our scans, then reconstructing through each channel's own
//! inverse should collapse a cast that grows with density; if our scanner's channel slopes
//! are not the datasheet's, it will not.
//!
//! **How it is measured without a neutral target.** We have no grey card in any frame, so
//! the probe never claims a frame is neutral. It bins pixels by red density and asks how
//! the blue-minus-red *log exposure ratio drifts across the bins*, after normalising at the
//! mid bin (which is what a white balance does). A scene-colour bias shifts all bins
//! together and cancels; only the drift is read. Both paths are measured on the identical
//! pixels of the identical frames, so the comparison is controlled even though neither
//! absolute number means anything on its own.
//!
//! Path A is `10^(contrast · D′)` — the shipped scalar-contrast reconstruction reduced to
//! its channel behaviour (toe and shoulder are shared by all three channels, so they cannot
//! create or remove a *ratio* drift). Path B is `10^(curve_c⁻¹(D′_c))`.
//!
//! Discipline matches `pipeline::shadow_metrics`: asset-dependent entry points are
//! `#[ignore]`d and skip with a message when `../nc-assets` is absent, and only derived
//! numbers are printed — never pixels.
//!
//! ```text
//! cargo test --release curve_probe::channel_drift -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use super::film_stock::curves::{STOCKS as PROBE_STOCKS, StockCurves};
use super::film_stock::{curves_for, invert};
use crate::algo::density::to_density;
use crate::types::{DensityParams, FilmBase, FilmStock, REFERENCE_CONTRAST};

/// Same ceiling as the shipped 6 GiB default: a frame that converts normally measures here.
const DECODE_BUDGET_BYTES: u64 = 6 * 1024 * 1024 * 1024;

/// Fraction trimmed from each edge before sampling. Real scans are laid out
/// `dark holder → thin inset rebate → picture`, and both non-picture regions would
/// otherwise dominate the lowest density bins.
const INTERIOR_INSET: f32 = 0.12;

/// Cap on sampled pixels per frame. Bin medians over ~1M samples are statistically
/// indistinguishable from the full population and keep the probe's transient small.
const MAX_SAMPLES: usize = 1 << 20;

/// Density bins, spanning the frame's own 2nd–98th red-density percentiles.
const BINS: usize = 10;

/// The fixture rolls that have a digitized datasheet, as
/// `(roll in the asset manifest, recipe stem, stock key in `curve_data`)`.
const FIXTURES: &[(&str, &str, &str)] = &[
    ("Ektar", "Ektar", "ektar-100"),
    ("Portra160-2026-07-22", "Portra160-2026-07-22", "portra-160"),
    ("Portra160", "Portra160", "portra-160"),
    ("Portra400", "Portra400", "portra-400"),
    ("Portra400-leica-flaw", "Portra400-leica-flaw", "portra-400"),
    ("2026-07-24-Gold200", "2026-07-24-Gold200", "gold-200"),
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn assets_root() -> Option<PathBuf> {
    let root = std::env::var("NC_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("../nc-assets"));
    root.join("manifest.json").is_file().then_some(root)
}

/// The roll's frozen film base, read from the committed verification recipe. Fails loudly:
/// a missing key means the recipe was not frozen as expected, which must not be defaulted.
fn frozen_base(recipe: &Path) -> FilmBase {
    let text =
        std::fs::read_to_string(recipe).unwrap_or_else(|e| panic!("{}: {e}", recipe.display()));
    let v: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", recipe.display()));
    let base = v["film_base"]["source"]["explicit"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: film_base.source.explicit missing", recipe.display()));
    let rgb: Vec<f32> = base.iter().map(|x| x.as_f64().unwrap() as f32).collect();
    assert_eq!(rgb.len(), 3, "{}: film base is not RGB", recipe.display());
    FilmBase {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
    }
}

/// The roll's `real` frames, in manifest order.
fn real_frames(assets: &Path, roll: &str) -> Vec<PathBuf> {
    let path = assets.join("manifest.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let m: serde_json::Value = serde_json::from_str(&text).unwrap();
    let frames = m["rolls"][roll]["frames"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: rolls.{roll}.frames missing", path.display()));
    let mut out: Vec<PathBuf> = frames
        .iter()
        .filter(|f| f["role"].as_str().unwrap_or("real") == "real")
        .map(|f| assets.join(f["file"].as_str().unwrap()))
        .collect();
    out.sort();
    out
}

fn stock(key: &str) -> &'static StockCurves {
    PROBE_STOCKS
        .iter()
        .find(|s| s.name == key)
        .unwrap_or_else(|| panic!("no digitized curve for stock {key}"))
}

fn median(v: &mut [f32]) -> f32 {
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    sorted[((sorted.len() - 1) as f32 * p) as usize]
}

/// One frame's drift measurement: the per-bin blue−red and green−red log2 exposure ratios
/// for both paths, already normalised so the middle bin reads zero.
struct Drift {
    d_r: [f32; BINS],
    scalar_b: [f32; BINS],
    scalar_g: [f32; BINS],
    curve_b: [f32; BINS],
    curve_g: [f32; BINS],
    out_of_range: f32,
    /// Least-squares slope of each drift row against `d_r`, in stops per unit density.
    /// This is the quantity the datasheet actually predicts, and it is far more robust
    /// than max−min: a single outlier bin (a coloured subject that happens to occupy one
    /// luminance range) moves a swing by its full excursion but a slope by little.
    slope_scalar_b: f32,
    slope_curve_b: f32,
    /// What the stock's own curves predict the scalar path's blue drift slope to be over
    /// *this frame's* density range. Measured scalar ≈ predicted means the datasheet's
    /// channel structure is present in the scan; measured ≪ predicted means it is not.
    slope_predicted_b: f32,
    /// The same three for **green**. Blue is where the effect is largest, but green is
    /// where a *residual* matters most perceptually — a green/magenta error has no
    /// "warm/cool" reading that the eye forgives, so a residual the blue channel would
    /// shrug off shows up as a cast.
    slope_scalar_g: f32,
    slope_curve_g: f32,
    slope_predicted_g: f32,
}

/// The identity per-channel density gain — **not** `DensityParams::default()`.
///
/// Every measurement here is differenced against a prediction derived from the *uncorrected*
/// datasheet tables, so "no correction" has to be spelled out. Since 2026-09-09 the
/// parametric default is `[1, 0.90, 0.86]`, so `DensityParams::default()` as a stand-in for
/// identity silently pre-corrects the scan half of that difference while the prediction half
/// stays uncorrected — the trap the crate records for probes that follow a shipped value
/// (`algo::density::tests::identity_gain` and `pipeline::stages::golden::frozen_density`
/// exist for the same reason). A site that *wants* the shipped gain states it as a literal
/// in a candidate table, where it is visible.
fn identity_gain() -> DensityParams {
    DensityParams {
        scale: [1.0, 1.0, 1.0],
        ..DensityParams::default()
    }
}

/// One frame's drift at the identity gain — the measurement [`channel_drift`] and
/// [`stock_table_variants`] difference against the datasheet's own prediction.
fn measure(frame: &Path, base: &FilmBase, sc: &StockCurves) -> Option<Drift> {
    measure_with(frame, base, sc, &identity_gain())
}

/// [`measure`] under an arbitrary [`DensityParams`], so a candidate per-channel correction
/// can be scored on the real render path rather than estimated from a linearisation.
fn measure_with(
    frame: &Path,
    base: &FilmBase,
    sc: &StockCurves,
    params: &DensityParams,
) -> Option<Drift> {
    let (image, _) = crate::io::decode::decode_within(frame, DECODE_BUDGET_BYTES).ok()?;
    measure_decoded(&image, base, sc, params)
}

/// [`measure_with`] on an already-decoded frame, so a sweep over candidate corrections
/// pays the decode once instead of once per candidate.
fn measure_decoded(
    image: &crate::types::LinearImage,
    base: &FilmBase,
    sc: &StockCurves,
    params: &DensityParams,
) -> Option<Drift> {
    let d = to_density(image, base, params);

    let (w, h) = (d.width as usize, d.height as usize);
    let inset_x = (w as f32 * INTERIOR_INSET) as usize;
    let inset_y = (h as f32 * INTERIOR_INSET) as usize;
    let interior: Vec<[f32; 3]> = (inset_y..h - inset_y)
        .flat_map(|y| (inset_x..w - inset_x).map(move |x| (y * w + x) * 3))
        .filter_map(|i| {
            let px = [d.density[i], d.density[i + 1], d.density[i + 2]];
            px.iter().all(|v| v.is_finite()).then_some(px)
        })
        .collect();
    let stride = (interior.len() / MAX_SAMPLES).max(1);
    let sample: Vec<[f32; 3]> = interior.into_iter().step_by(stride).collect();
    if sample.len() < BINS * 64 {
        return None;
    }

    let mut reds: Vec<f32> = sample.iter().map(|p| p[0]).collect();
    reds.sort_by(f32::total_cmp);
    let (lo, hi) = (percentile(&reds, 0.02), percentile(&reds, 0.98));
    if hi <= lo || !hi.is_finite() || !lo.is_finite() {
        return None;
    }

    // log2 exposure ratios per bin, both paths.
    let mut bins: Vec<Vec<[f32; 4]>> = vec![vec![]; BINS];
    let mut oor = 0usize;
    let ln2 = std::f32::consts::LN_2 / std::f32::consts::LN_10; // log10(2)
    for px in &sample {
        let t = (px[0] - lo) / (hi - lo);
        if !(0.0..1.0).contains(&t) {
            continue;
        }
        let b = ((t * BINS as f32) as usize).min(BINS - 1);

        // Path A: one scalar contrast for every channel, so the exposure ratio is just the
        // density difference scaled — this is what nc asserts today.
        let s_b = REFERENCE_CONTRAST * (px[2] - px[0]) / ln2;
        let s_g = REFERENCE_CONTRAST * (px[1] - px[0]) / ln2;

        // Path B: each channel through its own published inverse.
        let (er, ok_r) = invert(sc.channels[0], px[0]);
        let (eg, ok_g) = invert(sc.channels[1], px[1]);
        let (eb, ok_b) = invert(sc.channels[2], px[2]);
        if !(ok_r && ok_g && ok_b) {
            oor += 1;
        }
        bins[b].push([s_b, s_g, (eb - er) / ln2, (eg - er) / ln2]);
    }

    let mut out = Drift {
        d_r: [0.0; BINS],
        scalar_b: [0.0; BINS],
        scalar_g: [0.0; BINS],
        curve_b: [0.0; BINS],
        curve_g: [0.0; BINS],
        out_of_range: oor as f32 / sample.len() as f32,
        slope_scalar_b: 0.0,
        slope_curve_b: 0.0,
        slope_predicted_b: 0.0,
        slope_scalar_g: 0.0,
        slope_curve_g: 0.0,
        slope_predicted_g: 0.0,
    };
    for (i, bin) in bins.iter().enumerate() {
        if bin.is_empty() {
            return None;
        }
        out.d_r[i] = lo + (hi - lo) * (i as f32 + 0.5) / BINS as f32;
        for (k, slot) in [
            &mut out.scalar_b,
            &mut out.scalar_g,
            &mut out.curve_b,
            &mut out.curve_g,
        ]
        .into_iter()
        .enumerate()
        {
            let mut v: Vec<f32> = bin.iter().map(|r| r[k]).collect();
            slot[i] = median(&mut v);
        }
    }
    // Normalise at the middle bin: this is exactly what a white balance does, and it is
    // what makes the *drift* the only thing compared.
    let mid = BINS / 2;
    for slot in [
        &mut out.scalar_b,
        &mut out.scalar_g,
        &mut out.curve_b,
        &mut out.curve_g,
    ] {
        let m = slot[mid];
        for v in slot.iter_mut() {
            *v -= m;
        }
    }
    out.slope_scalar_b = slope(&out.d_r, &out.scalar_b);
    out.slope_curve_b = slope(&out.d_r, &out.curve_b);
    out.slope_predicted_b = predicted_slope(sc, 2, out.d_r[0], out.d_r[BINS - 1]);
    out.slope_scalar_g = slope(&out.d_r, &out.scalar_g);
    out.slope_curve_g = slope(&out.d_r, &out.curve_g);
    out.slope_predicted_g = predicted_slope(sc, 1, out.d_r[0], out.d_r[BINS - 1]);
    Some(out)
}

/// Least-squares slope of `y` against `x`.
fn slope(x: &[f32; BINS], y: &[f32; BINS]) -> f32 {
    let n = BINS as f32;
    let (mx, my) = (x.iter().sum::<f32>() / n, y.iter().sum::<f32>() / n);
    let num: f32 = x.iter().zip(y).map(|(a, b)| (a - mx) * (b - my)).sum();
    let den: f32 = x.iter().map(|a| (a - mx) * (a - mx)).sum();
    num / den
}

/// The blue drift slope the stock's own curves imply for the scalar-contrast path, over a
/// red-density span `[d_lo, d_hi]`.
///
/// On a neutral ramp the scalar path leaves `contrast · (D'_B − D'_R)`; differentiating
/// that against `D'_R` over the frame's own range is what the probe should recover if the
/// scan sits on the datasheet's density scale.
fn predicted_slope(sc: &StockCurves, channel: usize, d_lo: f32, d_hi: f32) -> f32 {
    let log2 = std::f32::consts::LN_2 / std::f32::consts::LN_10;
    let other_at = |d_red: f32| -> f32 {
        // Walk to the exposure that puts red at `d_red`, then read the other channel there.
        let (x, _) = invert(sc.channels[0], d_red);
        let table = sc.channels[channel];
        let i = table
            .partition_point(|p| p.0 <= x)
            .clamp(1, table.len() - 1);
        let ((x0, d0), (x1, d1)) = (table[i - 1], table[i]);
        d0 + (x - x0) * (d1 - d0) / (x1 - x0)
    };
    let lo = REFERENCE_CONTRAST * (other_at(d_lo) - d_lo) / log2;
    let hi = REFERENCE_CONTRAST * (other_at(d_hi) - d_hi) / log2;
    (hi - lo) / (d_hi - d_lo)
}

fn swing(v: &[f32; BINS]) -> f32 {
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &x in v {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    hi - lo
}

/// Where the out-of-table samples actually are: at the decoder's transmission floor (the
/// opaque film holder, which is not picture), or genuinely past the end of the published
/// curve (a real specular)?
///
/// The distinction decides whether the out-of-table warning is a false alarm on every
/// full-frame scan. Prints the interior/exterior split alongside it, since the holder is by
/// definition outside the picture.
#[test]
#[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
fn out_of_table_provenance() {
    let Some(assets) = assets_root() else {
        eprintln!("SKIP: no ../nc-assets/manifest.json (set NC_ASSETS to override)");
        return;
    };
    let recipes = repo_root().join("scripts/real-scan-verify/recipes");
    println!(
        "\n{:34}{:>10}{:>12}{:>12}{:>12}",
        "frame", "above%", "at floor%", "interior%", "edge%"
    );
    for (roll, stem, key) in FIXTURES {
        let base = frozen_base(&recipes.join(format!("{stem}.json")));
        let sc = stock(key);
        for frame in real_frames(&assets, roll).into_iter().take(2) {
            let name = frame.file_name().unwrap().to_string_lossy().to_string();
            let Ok((image, _)) = crate::io::decode::decode_within(&frame, DECODE_BUDGET_BYTES)
            else {
                continue;
            };
            // Identity, which is also what the shipped characteristic path resolves
            // (`DensityParams::default_scale_for(Characteristic)`) — this probe measures how
            // far a frame falls outside *that* path's tables.
            let d = to_density(&image, &base, &identity_gain());
            // The density a floored transmission produces, per channel. `to_density` clamps
            // transmission at 1e-6 before the log, so this is the exact value a dead or
            // opaque sample lands on — nothing darker can exist.
            let floor: [f32; 3] = [base.r, base.g, base.b].map(|b| -(1e-6f32 / b).log10());
            let top: [f32; 3] = std::array::from_fn(|c| sc.channels[c][sc.channels[c].len() - 1].1);
            let (w, h) = (d.width as usize, d.height as usize);
            let (ix, iy) = (
                (w as f32 * INTERIOR_INSET) as usize,
                (h as f32 * INTERIOR_INSET) as usize,
            );
            let (mut above, mut at_floor, mut interior_above, mut edge_above) =
                (0u64, 0u64, 0u64, 0u64);
            let total = (w * h) as f64;
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) * 3;
                    let over = (0..3).any(|c| d.density[i + c] > top[c]);
                    if !over {
                        continue;
                    }
                    above += 1;
                    if (0..3).any(|c| (d.density[i + c] - floor[c]).abs() < 1e-3) {
                        at_floor += 1;
                    }
                    if y >= iy && y < h - iy && x >= ix && x < w - ix {
                        interior_above += 1;
                    } else {
                        edge_above += 1;
                    }
                }
            }
            let pct = |n: u64| 100.0 * n as f64 / total;
            println!(
                "{name:34}{:10.2}{:12.2}{:12.2}{:12.2}",
                pct(above),
                pct(at_floor),
                pct(interior_above),
                pct(edge_above)
            );
        }
    }
    println!(
        "\n`at floor%` is the share of the frame sitting on the decoder's 1e-6 transmission \
         clamp\n— the opaque holder, which is not a measurement of anything."
    );
}

/// What the characteristic curve actually hands the display stage, measured on **picture
/// area only**.
///
/// The rendering choice turns on one number: how far above diffuse white real content
/// reaches. A whole-frame statistic cannot answer it — the nearly-opaque film holder sits
/// at the decoder's epsilon floor, so its corrected density is enormous and it reconstructs
/// to exposures no scene contains (`algo/auto-anchor-interior-measurement`). Trimming the
/// edges is what separates "this frame has speculars" from "this frame has a holder".
#[test]
#[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
fn exposure_range() {
    let Some(assets) = assets_root() else {
        eprintln!("SKIP: no ../nc-assets/manifest.json (set NC_ASSETS to override)");
        return;
    };
    let recipes = repo_root().join("scripts/real-scan-verify/recipes");
    println!(
        "\nReconstructed exposure on picture area, in stops relative to diffuse white \
         (1.0 = white,\n0 stops). Percentiles over the interior; `>W` is the fraction \
         above each candidate white point.\n"
    );
    println!(
        "{:34}{:>8}{:>8}{:>8}{:>8}{:>8}   {:>7}{:>7}{:>7}",
        "frame", "p50", "p99", "p99.9", "p99.99", "max", ">1x", ">4x", ">16x"
    );
    let mut worst_p9999: f32 = f32::NEG_INFINITY;
    for (roll, stem, key) in FIXTURES {
        let base = frozen_base(&recipes.join(format!("{stem}.json")));
        let sc = stock(key);
        for frame in real_frames(&assets, roll) {
            let name = frame.file_name().unwrap().to_string_lossy().to_string();
            let Ok((image, _)) = crate::io::decode::decode_within(&frame, DECODE_BUDGET_BYTES)
            else {
                continue;
            };
            // Identity: the exposure below is read through each stock's published inverse,
            // i.e. the characteristic path, whose own default gain is identity.
            let d = to_density(&image, &base, &identity_gain());
            let (w, h) = (d.width as usize, d.height as usize);
            let (ix, iy) = (
                (w as f32 * INTERIOR_INSET) as usize,
                (h as f32 * INTERIOR_INSET) as usize,
            );
            // Scene luminance of the reconstructed exposure, per pixel: the max channel is
            // what clips first, so it is what the display stage has to fit.
            let mut peaks: Vec<f32> = Vec::new();
            for y in iy..h - iy {
                for x in ix..w - ix {
                    let i = (y * w + x) * 3;
                    let mut peak = 0.0f32;
                    for (c, table) in sc.channels.iter().enumerate() {
                        let v = d.density[i + c];
                        if !v.is_finite() {
                            peak = f32::NAN;
                            break;
                        }
                        let (log_e, _) = invert(table, v);
                        peak = peak.max(10f32.powf(log_e));
                    }
                    if peak.is_finite() {
                        peaks.push(peak);
                    }
                }
            }
            if peaks.len() < 1000 {
                continue;
            }
            peaks.sort_by(f32::total_cmp);
            let stops = |v: f32| v.max(1e-9).log2();
            let pc = |p: f32| peaks[((peaks.len() - 1) as f32 * p) as usize];
            let over = |t: f32| {
                100.0 * peaks.iter().filter(|v| **v > t).count() as f32 / peaks.len() as f32
            };
            worst_p9999 = worst_p9999.max(stops(pc(0.9999)));
            println!(
                "{name:34}{:8.2}{:8.2}{:8.2}{:8.2}{:8.2}   {:6.2}%{:6.2}%{:6.2}%",
                stops(pc(0.5)),
                stops(pc(0.99)),
                stops(pc(0.999)),
                stops(pc(0.9999)),
                stops(peaks[peaks.len() - 1]),
                over(1.0),
                over(4.0),
                over(16.0),
            );
        }
    }
    println!(
        "\nworst p99.99 across every frame: {worst_p9999:.2} stops over diffuse white \
         — the headroom a display tone has to carry to keep 99.99% of picture content."
    );
}

/// The per-channel green scale that nulls each roll's measured green drift.
///
/// **Not a shippable correction** — it is fitted to our own scans, which is exactly what
/// the project forbids for a default. Its purpose is to bound what any *per-channel*
/// correction can achieve: if a render at this scale still reads green to the eye, then no
/// diagonal fix is sufficient and the cross-channel 3×3 is the only route, which is the
/// open question for `io/scanner-density-calibration`.
///
/// Measured on the real render path (via [`measure_with`]), not from a linearisation,
/// because the inverse curve's slope varies across the density range. A **grid** rather
/// than a bisection: each frame is decoded once and every candidate scored against it,
/// where bisecting would re-decode 50–160 MB per iteration.
#[test]
#[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
fn green_scale_that_nulls_the_residual() {
    let Some(assets) = assets_root() else {
        eprintln!("SKIP: no ../nc-assets/manifest.json (set NC_ASSETS to override)");
        return;
    };
    let recipes = repo_root().join("scripts/real-scan-verify/recipes");
    const GRID: [f32; 9] = [0.80, 0.85, 0.90, 0.925, 0.95, 0.975, 1.0, 1.05, 1.10];

    println!("\nMean green drift of the curve path at each candidate green density scale.\n");
    print!("{:26}", "roll");
    for g in GRID {
        print!("{g:>8.3}");
    }
    println!("{:>10}", "-> null");

    for (roll, stem, key) in FIXTURES {
        let base = frozen_base(&recipes.join(format!("{stem}.json")));
        let sc = stock(key);
        let mut sums = [0.0f32; GRID.len()];
        let mut n = 0.0f32;
        for frame in real_frames(&assets, roll) {
            let Ok((image, _)) = crate::io::decode::decode_within(&frame, DECODE_BUDGET_BYTES)
            else {
                continue;
            };
            let mut any = false;
            for (i, g) in GRID.iter().enumerate() {
                // `..identity_gain()` rather than `..DensityParams::default()`: the whole
                // `scale` is stated here, so the two are arithmetically identical, but the
                // base a candidate is spread over should not be a moving default.
                let params = DensityParams {
                    scale: [1.0, *g, 1.0],
                    ..identity_gain()
                };
                if let Some(m) = measure_decoded(&image, &base, sc, &params) {
                    sums[i] += slope(&m.d_r, &m.curve_g);
                    any = true;
                }
            }
            if any {
                n += 1.0;
            }
        }
        if n == 0.0 {
            continue;
        }
        let means: Vec<f32> = sums.iter().map(|s| s / n).collect();
        // Linear interpolation across the sign change; the drift is monotone in the scale.
        let mut null = f32::NAN;
        for i in 0..GRID.len() - 1 {
            if means[i] * means[i + 1] <= 0.0 && means[i] != means[i + 1] {
                let t = -means[i] / (means[i + 1] - means[i]);
                null = GRID[i] + t * (GRID[i + 1] - GRID[i]);
                break;
            }
        }
        print!("{roll:26}");
        for m in &means {
            print!("{m:>8.2}");
        }
        println!("{null:>10.3}");
    }
    println!("\nApply as `--density-scale 1,<null>,1` beside the stock's own curve.");
}

/// Whether a stock states no usable aim pair, so the aim-matched variant is undefined.
/// The 800-speed sheets tabulate Δ = 0.25 against their own curves' ~0.36, an established
/// inconsistency of a different kind, so "match the aim table" is meaningless for them.
fn sc_aims_missing(key: &str) -> bool {
    matches!(key, "portra-800" | "ultramax-800" | "generic-c41")
}

/// Candidate adjustments to one stock's tables, scored against real frames.
///
/// The residual is `scan divergence − what the tables predict`, and the scan half does not
/// depend on the tables at all (it is `contrast · (D′_G − D′_R)`, pure measurement). So a
/// candidate can be scored exactly without re-rendering anything: only the predicted half
/// moves. That is what makes this cheap enough to sweep.
///
/// Ektar 100 is the case that motivated it — its published curve predicts the *least* green
/// divergence of the corpus (+0.22) where its scans show the *most* (+1.26), and its aim
/// table disagrees with its own curve by 11%.
#[test]
#[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
fn stock_table_variants() {
    let Some(assets) = assets_root() else {
        eprintln!("SKIP: no ../nc-assets/manifest.json (set NC_ASSETS to override)");
        return;
    };
    let recipes = repo_root().join("scripts/real-scan-verify/recipes");

    // Rebuild a stock's tables under a candidate rule. Returned owned, so the probe can
    // score shapes that are not in the shipped registry.
    let scaled_red = |sc: &StockCurves, k: f32| -> Vec<Vec<(f32, f32)>> {
        // Scale red's *density* axis by `k`, leaving exposure and the other layers alone:
        // this is "make red's rise over the aim interval match the aim table".
        vec![
            sc.channels[0].iter().map(|(x, d)| (*x, d * k)).collect(),
            sc.channels[1].to_vec(),
            sc.channels[2].to_vec(),
        ]
    };
    let corpus_relationship = |sc: &StockCurves| -> Vec<Vec<(f32, f32)>> {
        // Keep this stock's own red curve; rebuild G and B as red plus the *generic's*
        // channel offsets at matched exposure. Uses the corpus consensus only for the
        // relationship its own sheet reports anomalously.
        let generic = curves_for(FilmStock::GenericC41);
        let at = |t: &[(f32, f32)], x: f32| -> f32 {
            let i = t.partition_point(|p| p.0 <= x).clamp(1, t.len() - 1);
            let ((x0, d0), (x1, d1)) = (t[i - 1], t[i]);
            d0 + (x - x0) * (d1 - d0) / (x1 - x0)
        };
        let mut out = vec![sc.channels[0].to_vec()];
        for c in 1..3 {
            out.push(
                sc.channels[0]
                    .iter()
                    .map(|(x, d)| {
                        (
                            *x,
                            d + (at(generic.channels[c], *x) - at(generic.channels[0], *x)),
                        )
                    })
                    .collect(),
            );
        }
        out
    };

    // Predicted green drift slope for an arbitrary table set, over a density span.
    let predicted = |tables: &[Vec<(f32, f32)>], d_lo: f32, d_hi: f32| -> f32 {
        let log2 = std::f32::consts::LN_2 / std::f32::consts::LN_10;
        let green_at = |d_red: f32| -> f32 {
            let (x, _) = invert(&tables[0], d_red);
            let t = &tables[1];
            let i = t.partition_point(|p| p.0 <= x).clamp(1, t.len() - 1);
            let ((x0, d0), (x1, d1)) = (t[i - 1], t[i]);
            d0 + (x - x0) * (d1 - d0) / (x1 - x0)
        };
        let lo = REFERENCE_CONTRAST * (green_at(d_lo) - d_lo) / log2;
        let hi = REFERENCE_CONTRAST * (green_at(d_hi) - d_hi) / log2;
        (hi - lo) / (d_hi - d_lo)
    };

    for (roll, stem, key) in FIXTURES {
        // Every fixture stock, not just Ektar: a correction that only helps the stock it
        // was invented for is a fudge, and the sweep is what tells the two apart.
        if sc_aims_missing(key) {
            continue;
        }
        let base = frozen_base(&recipes.join(format!("{stem}.json")));
        let sc = stock(key);
        let generic = curves_for(FilmStock::GenericC41);
        let owned_generic: Vec<Vec<(f32, f32)>> =
            generic.channels.iter().map(|c| c.to_vec()).collect();
        let owned_published: Vec<Vec<(f32, f32)>> =
            sc.channels.iter().map(|c| c.to_vec()).collect();
        // The aim table gives Δ = 0.36 where the curve rises 0.401 over the same interval,
        // so matching it means scaling red's density by 0.36/0.401.
        let aim_k = {
            let [grey, white] = sc.aims.expect("ektar has aims");
            let t = sc.channels[0];
            let at = |x: f32| {
                let i = t.partition_point(|p| p.0 <= x).clamp(1, t.len() - 1);
                let ((x0, d0), (x1, d1)) = (t[i - 1], t[i]);
                d0 + (x - x0) * (d1 - d0) / (x1 - x0)
            };
            let log18 = 0.18f32.log10();
            (white - grey) / (at(log18 + 0.694) - at(log18))
        };

        println!("\n=== {roll} ({key}) — green drift, stops per unit density");
        println!("    red-density scale that matches the aim table: {aim_k:.3}\n");
        println!(
            "{:26}{:>9}{:>12}{:>12}{:>12}{:>12}",
            "frame", "scan", "published", "aim-matched", "corpus-rel", "generic"
        );
        let mut sums = [0.0f32; 5];
        let mut n = 0.0f32;
        for frame in real_frames(&assets, roll) {
            let name = frame.file_name().unwrap().to_string_lossy().to_string();
            let Some(m) = measure(&frame, &base, sc) else {
                continue;
            };
            let (d_lo, d_hi) = (m.d_r[0], m.d_r[BINS - 1]);
            let variants = [
                predicted(&owned_published, d_lo, d_hi),
                predicted(&scaled_red(sc, aim_k), d_lo, d_hi),
                predicted(&corpus_relationship(sc), d_lo, d_hi),
                predicted(&owned_generic, d_lo, d_hi),
            ];
            let scan = m.slope_scalar_g;
            println!(
                "{name:26}{scan:>9.2}{:>12.2}{:>12.2}{:>12.2}{:>12.2}",
                scan - variants[0],
                scan - variants[1],
                scan - variants[2],
                scan - variants[3],
            );
            sums[0] += scan;
            for (i, v) in variants.iter().enumerate() {
                sums[i + 1] += scan - v;
            }
            n += 1.0;
        }
        if n > 0.0 {
            println!(
                "{:26}{:>9.2}{:>12.2}{:>12.2}{:>12.2}{:>12.2}   <- residual (0 = neutral)",
                "MEAN",
                sums[0] / n,
                sums[1] / n,
                sums[2] / n,
                sums[3] / n,
                sums[4] / n
            );
        }
    }
}

#[test]
#[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
fn channel_drift() {
    let Some(assets) = assets_root() else {
        eprintln!("SKIP: no ../nc-assets/manifest.json (set NC_ASSETS to override)");
        return;
    };
    let recipes = repo_root().join("scripts/real-scan-verify/recipes");

    println!(
        "\nBlue−red exposure drift across the tone scale, normalised at the middle bin.\n\
         Units are stops. 'scalar' = one contrast for all channels (what nc does today);\n\
         'curve' = each channel through its own published inverse. Lower swing is better.\n"
    );
    struct Row {
        roll: &'static str,
        stock: &'static str,
        frame: String,
        swing_scalar_b: f32,
        swing_curve_b: f32,
        swing_scalar_g: f32,
        swing_curve_g: f32,
        slope_scalar_b: f32,
        slope_curve_b: f32,
        slope_predicted_b: f32,
        slope_scalar_g: f32,
        slope_curve_g: f32,
        slope_predicted_g: f32,
    }
    let mut totals: Vec<Row> = vec![];

    for (roll, stem, key) in FIXTURES {
        let base = frozen_base(&recipes.join(format!("{stem}.json")));
        let sc = stock(key);
        let frames = real_frames(&assets, roll);
        if frames.is_empty() {
            println!("{roll}: no real frames in the manifest");
            continue;
        }
        println!(
            "=== {roll}  ({}, {} {})  datasheet D-min {:?}",
            sc.name, sc.publication, sc.revision, sc.d_min
        );
        for frame in frames {
            let name = frame.file_name().unwrap().to_string_lossy().to_string();
            let Some(m) = measure(&frame, &base, sc) else {
                println!("  {name}: skipped (decode failed or too few interior samples)");
                continue;
            };
            println!("  {name}   out-of-table {:.3}%", m.out_of_range * 100.0);
            print!("      D'_R   ");
            for v in m.d_r {
                print!("{v:7.3}");
            }
            for (label, row) in [
                ("scalar B", &m.scalar_b),
                ("curve  B", &m.curve_b),
                ("scalar G", &m.scalar_g),
                ("curve  G", &m.curve_g),
            ] {
                print!("\n      {label}");
                for v in row {
                    print!("{v:7.2}");
                }
            }
            println!(
                "\n      swing  B: scalar {:.2} -> curve {:.2} stops   G: scalar {:.2} -> \
                 curve {:.2}\n      slope  B: scalar {:+.2} -> curve {:+.2} \
                 stops/density   (datasheet predicts {:+.2})",
                swing(&m.scalar_b),
                swing(&m.curve_b),
                swing(&m.scalar_g),
                swing(&m.curve_g),
                m.slope_scalar_b,
                m.slope_curve_b,
                m.slope_predicted_b,
            );
            totals.push(Row {
                roll,
                stock: sc.name,
                frame: name,
                swing_scalar_b: swing(&m.scalar_b),
                swing_curve_b: swing(&m.curve_b),
                swing_scalar_g: swing(&m.scalar_g),
                swing_curve_g: swing(&m.curve_g),
                slope_scalar_b: m.slope_scalar_b,
                slope_curve_b: m.slope_curve_b,
                slope_predicted_b: m.slope_predicted_b,
                slope_scalar_g: m.slope_scalar_g,
                slope_curve_g: m.slope_curve_g,
                slope_predicted_g: m.slope_predicted_g,
            });
        }
    }

    if totals.is_empty() {
        println!("no frames measured");
        return;
    }
    let n = totals.len() as f32;
    let mean = |f: fn(&Row) -> f32| -> f32 { totals.iter().map(f).sum::<f32>() / n };
    println!(
        "\nMEAN over {} frames\n  swing  blue: scalar {:.2} -> curve {:.2} stops ({:+.0}%);  \
         green: scalar {:.2} -> curve {:.2} ({:+.0}%)",
        totals.len(),
        mean(|t| t.swing_scalar_b),
        mean(|t| t.swing_curve_b),
        100.0 * (mean(|t| t.swing_curve_b) / mean(|t| t.swing_scalar_b) - 1.0),
        mean(|t| t.swing_scalar_g),
        mean(|t| t.swing_curve_g),
        100.0 * (mean(|t| t.swing_curve_g) / mean(|t| t.swing_scalar_g) - 1.0),
    );
    println!(
        "  slope  blue: scalar {:+.2} -> curve {:+.2} stops/density; datasheet predicts \
         {:+.2}\n         => the scan carries {:.0}% of the divergence the datasheet claims",
        mean(|t| t.slope_scalar_b),
        mean(|t| t.slope_curve_b),
        mean(|t| t.slope_predicted_b),
        100.0 * (mean(|t| t.slope_scalar_b) - mean(|t| t.slope_curve_b))
            / mean(|t| t.slope_predicted_b),
    );
    println!(
        "  slope  green: scalar {:+.2} -> curve {:+.2} stops/density; datasheet predicts {:+.2}",
        mean(|t| t.slope_scalar_g),
        mean(|t| t.slope_curve_g),
        mean(|t| t.slope_predicted_g),
    );
    println!("  per ROLL, with the frame-to-frame scatter of the residual:");
    println!(
        "    {:24}{:>4}{:>10}{:>10}{:>10}   per-frame residuals",
        "roll", "n", "residual", "sd", "sem"
    );
    for (roll, _, _) in FIXTURES {
        let rows: Vec<&Row> = totals.iter().filter(|t| t.roll == *roll).collect();
        if rows.is_empty() {
            continue;
        }
        let v: Vec<f32> = rows.iter().map(|t| t.slope_curve_g).collect();
        let n = v.len() as f32;
        let mean = v.iter().sum::<f32>() / n;
        let sd =
            (v.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / (n - 1.0).max(1.0)).sqrt();
        let each: Vec<String> = v.iter().map(|x| format!("{x:+.2}")).collect();
        println!(
            "    {:24}{:>4}{:>10.2}{:>10.2}{:>10.2}   {}",
            roll,
            v.len(),
            mean,
            sd,
            sd / n.sqrt(),
            each.join(" ")
        );
    }
    println!("  per ROLL (residual slope after inversion, stops/density — 0 is neutral):");
    println!(
        "    {:24}{:>4}{:>9}{:>11}{:>11}",
        "roll", "n", "green", "predicted", "residual"
    );
    for (roll, _, key) in FIXTURES {
        let rows: Vec<&Row> = totals.iter().filter(|t| t.roll == *roll).collect();
        if rows.is_empty() {
            continue;
        }
        let k = rows.len() as f32;
        let m = |f: fn(&&Row) -> f32| rows.iter().map(f).sum::<f32>() / k;
        println!(
            "    {:24}{:>4}{:>9.2}{:>11.2}{:>11.2}   ({key})",
            roll,
            rows.len(),
            m(|t| t.slope_scalar_g),
            m(|t| t.slope_predicted_g),
            m(|t| t.slope_curve_g),
        );
    }
    println!("  per stock (residual slope after inversion, stops/density — 0 is neutral):");
    for sc in PROBE_STOCKS {
        let rows: Vec<&Row> = totals.iter().filter(|t| t.stock == sc.name).collect();
        if rows.is_empty() {
            continue;
        }
        let k = rows.len() as f32;
        let m = |f: fn(&&Row) -> f32| rows.iter().map(f).sum::<f32>() / k;
        println!(
            "    {:14} n={:2}  green: scan {:+.2}  datasheet predicts {:+.2}  residual {:+.2}   \
             |  blue: scan {:+.2} predicts {:+.2} residual {:+.2}",
            sc.name,
            rows.len(),
            m(|t| t.slope_scalar_g),
            m(|t| t.slope_predicted_g),
            m(|t| t.slope_curve_g),
            m(|t| t.slope_scalar_b),
            m(|t| t.slope_predicted_b),
            m(|t| t.slope_curve_b),
        );
    }
    let worse: Vec<&str> = totals
        .iter()
        .filter(|t| t.swing_curve_b > t.swing_scalar_b)
        .map(|t| t.frame.as_str())
        .collect();
    println!(
        "  frames where the curve's blue swing is worse: {}/{}{}",
        worse.len(),
        totals.len(),
        if worse.is_empty() {
            String::new()
        } else {
            format!(" ({})", worse.join(", "))
        }
    );
}

/// **What per-channel `density.scale` should the sigmoid default to?**
///
/// The scalar path leaves `contrast · (D'_c − D'_R)`, so with `D'_c = s_c · D_c` each
/// channel's drift against red density is **exactly linear in that channel's own scale**:
/// `drift_c(s) = (contrast / log10 2) · (s_c · r_c − 1)`, where `r_c = dD_c/dD_R` is the
/// scan's own slope ratio. Two consequences shape this probe:
///
/// - One measurement per frame determines `r_c`, so *any* candidate scale is then
///   evaluated in closed form. The model is verified against a real re-measurement at two
///   candidates on every frame rather than assumed.
/// - **The corpus-nulling scale is `1 / mean(r_c)`, not `mean(1 / r_c)`.** An earlier
///   version of this probe averaged the per-frame nulling scales and reported 0.912/0.904;
///   by Jensen those are biased high, and applying them left blue drifting +0.48. The
///   corpus nulls at 0.899/0.845.
///
/// The figure of merit is **not** each channel's drift magnitude. `G−R` and `B−R` are not
/// perceptually independent: when both tilt together the result reads as a
/// colour-temperature drift (warm shadows, cool highlights), which the eye attributes to
/// the light. The unforgiving axis is green–magenta, `(G−R) − (B−R)/2`, so that is what a
/// candidate has to be judged on — a scale can shrink both per-channel drifts while making
/// this one worse, which is exactly what the datasheet-derived scale does.
///
/// Flat level is deliberately not reported: measured through real renders, the candidate
/// moves the per-channel output mean by ≤0.01 stop (green) and ≤0.06 (blue) because the
/// reconstruction's anchoring absorbs it. Only the tilt reaches the picture.
///
/// ```text
/// cargo test --release curve_probe::sigmoid_scale -- --ignored --nocapture
/// ```
#[test]
#[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
fn sigmoid_scale() {
    let Some(assets) = assets_root() else {
        eprintln!("SKIP: no ../nc-assets/manifest.json (set NC_ASSETS to override)");
        return;
    };
    let recipes = repo_root().join("scripts/real-scan-verify/recipes");
    let log2 = std::f32::consts::LN_2 / std::f32::consts::LN_10;
    let k = REFERENCE_CONTRAST / log2;

    /// Per-frame: the scan's own slope ratio for green and blue against red.
    struct Frame {
        roll: &'static str,
        r_g: f32,
        r_b: f32,
    }
    let mut frames_out: Vec<Frame> = vec![];
    // Where the model is checked against a real re-measurement.
    let mut worst_model_err = 0.0f32;

    for (roll, stem, key) in FIXTURES {
        let base = frozen_base(&recipes.join(format!("{stem}.json")));
        let sc = stock(key);
        for frame in real_frames(&assets, roll) {
            let Ok((image, _)) = crate::io::decode::decode_within(&frame, DECODE_BUDGET_BYTES)
            else {
                continue;
            };
            // **Must be the identity gain, not `DensityParams::default()`** — the line below
            // inverts `drift(1) = k·(r − 1)`, so anything but `s = 1` here recovers a wrong
            // `r` and every candidate in the table is then scored against it.
            let Some(at_one) = measure_decoded(&image, &base, sc, &identity_gain()) else {
                continue;
            };
            // r from the s = 1 measurement: drift(1) = k·(r − 1).
            let ratio = |drift: f32| drift / k + 1.0;
            let (r_g, r_b) = (ratio(at_one.slope_scalar_g), ratio(at_one.slope_scalar_b));
            // Falsify the closed form on this very frame, at two scales far apart.
            for probe in [[1.0, 0.977, 0.860], [1.0, 0.90, 0.86]] {
                let params = DensityParams {
                    scale: probe,
                    ..identity_gain()
                };
                let Some(real) = measure_decoded(&image, &base, sc, &params) else {
                    continue;
                };
                for (predicted, measured) in [
                    (k * (probe[1] * r_g - 1.0), real.slope_scalar_g),
                    (k * (probe[2] * r_b - 1.0), real.slope_scalar_b),
                ] {
                    worst_model_err = worst_model_err.max((predicted - measured).abs());
                }
            }
            frames_out.push(Frame { roll, r_g, r_b });
        }
    }
    if frames_out.is_empty() {
        println!("no frames measured");
        return;
    }
    let n = frames_out.len() as f32;
    let mean_r_g = frames_out.iter().map(|f| f.r_g).sum::<f32>() / n;
    let mean_r_b = frames_out.iter().map(|f| f.r_b).sum::<f32>() / n;
    assert!(
        worst_model_err < 0.02,
        "the closed form disagreed with a real render by {worst_model_err:.3} stops/density; \
         the table below would be modelling something nc does not do"
    );

    let drift = |s: f32, r: f32| k * (s * r - 1.0);
    let gm = |sg: f32, sb: f32, f: &Frame| drift(sg, f.r_g) - drift(sb, f.r_b) / 2.0;
    let summarize = |sg: f32, sb: f32| -> (f32, f32, f32, f32, usize) {
        let g = frames_out.iter().map(|f| drift(sg, f.r_g)).sum::<f32>() / n;
        let b = frames_out.iter().map(|f| drift(sb, f.r_b)).sum::<f32>() / n;
        let m = frames_out.iter().map(|f| gm(sg, sb, f)).sum::<f32>() / n;
        let a = frames_out.iter().map(|f| gm(sg, sb, f).abs()).sum::<f32>() / n;
        // How many of the six rolls sit within a quarter stop per density of neutral on the
        // unforgiving axis — a corpus mean can null while every roll is still off.
        let ok = FIXTURES
            .iter()
            .filter(|(roll, _, _)| {
                let rs: Vec<&Frame> = frames_out.iter().filter(|f| f.roll == *roll).collect();
                !rs.is_empty() && {
                    let c = rs.len() as f32;
                    (rs.iter().map(|f| gm(sg, sb, f)).sum::<f32>() / c).abs() < 0.25
                }
            })
            .count();
        (g, b, m, a, ok)
    };

    println!(
        "\nScalar (sigmoid) path drift, stops per unit density; 0 = neutral. {} frames, \
         {} rolls.\nClosed form verified against real renders to {:.4} stops/density.\n",
        frames_out.len(),
        FIXTURES.len(),
        worst_model_err
    );
    println!("  scan slope ratio vs red: green {mean_r_g:.4}, blue {mean_r_b:.4}");
    println!(
        "  => corpus-nulling scale: green {:.3}, blue {:.3}\n",
        1.0 / mean_r_g,
        1.0 / mean_r_b
    );
    println!(
        "  {:28}{:>9}{:>9}{:>12}{:>10}{:>10}",
        "scale [R,G,B]", "green", "blue", "green-mag", "|g-m|", "rolls<.25"
    );
    let candidates: [(&str, [f32; 3]); 6] = [
        ("shipped", [1.0, 1.0, 1.0]),
        ("datasheet generic", [1.0, 0.977, 0.860]),
        ("proposed", [1.0, 0.90, 0.86]),
        ("corpus null", [1.0, 1.0 / mean_r_g, 1.0 / mean_r_b]),
        ("green-magenta optimum", [1.0, 0.0, 0.0]), // filled below
        ("equal-slope", [1.0, 0.905, 0.905]),
    ];
    // The green scale that nulls the green-magenta axis for a *given* blue scale: solve
    // `k(s_g·r_g − 1) = k(s_b·r_b − 1)/2` for `s_g`. Not the same as nulling green.
    let gm_optimal_green = |sb: f32| (1.0 + (sb * mean_r_b - 1.0) / 2.0) / mean_r_g;
    for (name, scale) in candidates {
        let (sg, sb) = if name == "green-magenta optimum" {
            (gm_optimal_green(0.860), 0.860)
        } else {
            (scale[1], scale[2])
        };
        let (g, b, m, a, ok) = summarize(sg, sb);
        println!(
            "  {:28}{:>+9.2}{:>+9.2}{:>+12.2}{:>10.2}{:>8}/{}",
            format!("{name} [1, {sg:.3}, {sb:.3}]"),
            g,
            b,
            m,
            a,
            ok,
            FIXTURES.len()
        );
    }

    println!("\n  per roll, on the unforgiving axis (green-magenta), by candidate:");
    println!(
        "    {:24}{:>10}{:>12}{:>11}{:>13}",
        "roll", "shipped", "datasheet", "proposed", "corpus null"
    );
    for (roll, _, _) in FIXTURES {
        let rs: Vec<&Frame> = frames_out.iter().filter(|f| f.roll == *roll).collect();
        if rs.is_empty() {
            continue;
        }
        let c = rs.len() as f32;
        let at = |sg: f32, sb: f32| rs.iter().map(|f| gm(sg, sb, f)).sum::<f32>() / c;
        println!(
            "    {:24}{:>+10.2}{:>+12.2}{:>+11.2}{:>+13.2}",
            roll,
            at(1.0, 1.0),
            at(0.977, 0.860),
            at(0.90, 0.86),
            at(1.0 / mean_r_g, 1.0 / mean_r_b),
        );
    }
    println!(
        "\n  `rolls<.25` is the column that decides whether a *generic* scale is worth\n  \
         shipping: a corpus mean can read neutral while every roll is off, and the\n  \
         per-roll table shows the residual scatter a single constant cannot remove."
    );
}

/// Does the new `density.scale` default help or hurt the **characteristic** path, where
/// the per-channel structure is already carried by each stock's own inverted curves?
///
/// `to_density` applies the gain *before* the curve, so the two corrections compose rather
/// than one superseding the other. That is only right if the published curves under-correct
/// by about the amount the gain supplies — which is what `channel_drift` measured (49% of
/// the real green drift, 98% of blue). This checks it on the render path instead of
/// assuming it.
///
/// ```text
/// cargo test --release curve_probe::scale_against_the_characteristic_curve -- --ignored --nocapture
/// ```
#[test]
#[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
fn scale_against_the_characteristic_curve() {
    let Some(assets) = assets_root() else {
        eprintln!("SKIP: no ../nc-assets/manifest.json (set NC_ASSETS to override)");
        return;
    };
    let recipes = repo_root().join("scripts/real-scan-verify/recipes");
    let candidates: [(&str, [f32; 3]); 5] = [
        ("identity", [1.0, 1.0, 1.0]),
        ("new default", [1.0, 0.90, 0.86]),
        ("blue only", [1.0, 1.0, 0.86]),
        // Which way round is the aim-matched red scale? `stock_table_variants` prints the
        // factor that scales the **table's** red density (Ektar 0.898); `--density-scale`
        // multiplies the **scan's** density before the table is inverted, so the two are
        // reciprocal and picking the wrong one doubles the error instead of removing it.
        // Both are measured here rather than derived.
        // **Measured answer: the reciprocal is the right one.** Table-side 0.898 takes
        // green–magenta to +0.72 (worse than identity's +0.35); the reciprocal 1.114 takes
        // it to **+0.01**, the flattest of anything measured on this path. Both rows are
        // kept because the direction is not recoverable by reading either number.
        ("aim red, table-side (wrong way)", [0.898, 1.0, 1.0]),
        ("aim red, as --density-scale", [1.114, 1.0, 1.0]),
    ];
    println!(
        "\nCharacteristic (per-stock curve) path drift, stops per unit density; 0 = neutral.\n\
         The gain is applied before the curve, so it composes with the curve's own\n\
         per-channel correction rather than replacing it.\n"
    );
    println!(
        "  {:32}{:>17}{:>9}{:>9}{:>12}",
        "config", "scale", "green", "blue", "green-mag"
    );
    let mut agg: Vec<(usize, f32, f32)> = vec![(0, 0.0, 0.0); candidates.len()];
    for (roll, stem, key) in FIXTURES {
        let base = frozen_base(&recipes.join(format!("{stem}.json")));
        let sc = stock(key);
        for frame in real_frames(&assets, roll) {
            let Ok((image, _)) = crate::io::decode::decode_within(&frame, DECODE_BUDGET_BYTES)
            else {
                continue;
            };
            for (i, (_, scale)) in candidates.iter().enumerate() {
                // Each candidate states its whole `scale`, the shipped one included (the
                // `new default` row), so the base carries only the neutral offsets.
                let params = DensityParams {
                    scale: *scale,
                    ..identity_gain()
                };
                if let Some(m) = measure_decoded(&image, &base, sc, &params) {
                    agg[i].0 += 1;
                    agg[i].1 += m.slope_curve_g;
                    agg[i].2 += m.slope_curve_b;
                }
            }
        }
    }
    for (i, (name, scale)) in candidates.iter().enumerate() {
        let (n, g, b) = agg[i];
        if n == 0 {
            continue;
        }
        let (g, b) = (g / n as f32, b / n as f32);
        println!(
            "  {:32}{:>17}{:>+9.2}{:>+9.2}{:>+12.2}",
            name,
            format!("[{:.3},{:.2},{:.2}]", scale[0], scale[1], scale[2]),
            g,
            b,
            g - b / 2.0
        );
    }
}

/// The whole-roll sets: one roll each of two stocks, every frame of each, all on the same
/// scanner and SilverFast build. Deliberately **not** [`FIXTURES`] — that list is the
/// calibration corpus behind the shipped default, and these carry 32 frames apiece, which
/// would swamp it.
const WHOLE_ROLLS: &[(&str, &str)] = &[
    ("2026-09-09-Ektar", "ektar-100"),
    ("2026-09-11-Portra400", "portra-400"),
];

/// Minimum red-density span for a frame's slope fit to be conditioned. A frame that spans
/// no density cannot carry either measurement below: the slope fit is ill-posed, and
/// "near-white" is meaningless on a uniform field. Chosen from the measured distribution,
/// not from theory.
///
/// **It does not, and cannot, exclude a calibration frame.** `2026-09-11-Portra400`'s frame
/// `1639` is half leader and half base and measured the *largest* span on that roll (1.40) —
/// base-to-leader spans the whole density range. What it lacks is **scene** content, which no
/// span threshold can see. Such frames are excluded by carrying a role other than `real` in
/// the manifest, which is what [`real_frames`] filters on; a span filter is not a substitute
/// and assuming it was let that frame into a first run of these statistics.
const MIN_SPAN: f32 = 0.35;

/// One frame's reading on a whole-roll set.
struct RollRow {
    stem: String,
    /// The scan's own slope ratio against red, per channel.
    r_g: f32,
    r_b: f32,
    /// Residual drift left by the **characteristic** path — each channel's own published
    /// curve inverted — in stops per unit density, 0 neutral. The scalar path's equivalent
    /// is `k·(r − 1)`; the difference between them is what the stock's tables removed.
    curve_g: f32,
    curve_b: f32,
    /// Per-channel density relative to red at the near-white percentile, and the
    /// grey-world control.
    w_r: f32,
    w_g: f32,
    w_b: f32,
    g_g: f32,
    g_b: f32,
    span: f32,
    oor: f32,
}

/// Near-white percentile. `WbSource::Percentile` equalises the channels at a matched high
/// percentile; this reads the same statistic without the print-stage round trip.
const NEAR_WHITE: f32 = 0.995;

/// Measure every `real` frame of one roll, at the identity gain.
///
/// The film base is the roll's own frozen calibration, so both a slope and a *level* can be
/// read off it. That matters: the slope is immune to a base error (the per-channel constant
/// cancels in `measure_decoded`'s middle-bin normalisation) but the level is exactly what a
/// base error corrupts.
/// What a roll offered and what survived. Printed, because three independent `continue`s
/// below can silently shrink the set and a partially-measured roll would otherwise print the
/// same confident summary as a complete one.
struct RollMeasurement {
    rows: Vec<RollRow>,
    offered: usize,
    dropped: Vec<(String, &'static str)>,
}

impl RollMeasurement {
    /// One line naming every frame that did not make it, or saying none did not.
    fn report_drops(&self) {
        if self.dropped.is_empty() {
            println!("  all {} manifest `real` frames measured", self.offered);
            return;
        }
        println!(
            "  measured {} of {} manifest `real` frames; dropped:",
            self.rows.len(),
            self.offered
        );
        for (stem, why) in &self.dropped {
            println!("    {stem:28}{why}");
        }
    }
}

fn measure_roll(assets: &Path, roll: &str, stock_key: &str) -> RollMeasurement {
    let recipes = repo_root().join("scripts/real-scan-verify/recipes");
    let base = frozen_base(&recipes.join(format!("{roll}.json")));
    let sc = stock(stock_key);
    let log2 = std::f32::consts::LN_2 / std::f32::consts::LN_10;
    let k = REFERENCE_CONTRAST / log2;

    let frames = real_frames(assets, roll);
    let offered = frames.len();
    let mut rows = vec![];
    let mut dropped: Vec<(String, &'static str)> = vec![];
    for frame in frames {
        let stem = frame
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let Ok((image, _)) = crate::io::decode::decode_within(&frame, DECODE_BUDGET_BYTES) else {
            dropped.push((stem, "decode failed"));
            continue;
        };
        // **Identity gain, not `DensityParams::default()`** — the inversion below is
        // `drift(1) = k·(r − 1)`, so any other scale recovers a wrong `r`.
        let Some(at_one) = measure_decoded(&image, &base, sc, &identity_gain()) else {
            // `measure_decoded` declines on too-few samples, a degenerate red range, or any
            // empty bin — the last overlaps the span filter, so both rules must be visible.
            dropped.push((stem, "measure_decoded declined (sparse or degenerate)"));
            continue;
        };
        let ratio = |drift: f32| drift / k + 1.0;

        let d = to_density(&image, &base, &identity_gain());
        let (w, h) = (d.width as usize, d.height as usize);
        let (ix, iy) = (
            (w as f32 * INTERIOR_INSET) as usize,
            (h as f32 * INTERIOR_INSET) as usize,
        );
        let interior: Vec<[f32; 3]> = (iy..h - iy)
            .flat_map(|y| (ix..w - ix).map(move |x| (y * w + x) * 3))
            .filter_map(|i| {
                let px = [d.density[i], d.density[i + 1], d.density[i + 2]];
                px.iter().all(|v| v.is_finite()).then_some(px)
            })
            .collect();
        let stride = (interior.len() / MAX_SAMPLES).max(1);
        let sample: Vec<[f32; 3]> = interior.into_iter().step_by(stride).collect();
        if sample.len() < 4096 {
            dropped.push((stem, "too few finite interior samples"));
            continue;
        }
        let n = sample.len() as f32;
        let chan = |c: usize| -> (f32, f32) {
            let mut v: Vec<f32> = sample.iter().map(|p| p[c]).collect();
            v.sort_by(f32::total_cmp);
            (percentile(&v, NEAR_WHITE), v.iter().sum::<f32>() / n)
        };
        let (wr, mr) = chan(0);
        let (wg, mg) = chan(1);
        let (wb, mb) = chan(2);

        rows.push(RollRow {
            stem,
            r_g: ratio(at_one.slope_scalar_g),
            r_b: ratio(at_one.slope_scalar_b),
            curve_g: at_one.slope_curve_g,
            curve_b: at_one.slope_curve_b,
            w_r: wr,
            w_g: wg - wr,
            w_b: wb - wr,
            g_g: mg - mr,
            g_b: mb - mr,
            span: at_one.d_r[BINS - 1] - at_one.d_r[0],
            oor: at_one.out_of_range,
        });
    }
    RollMeasurement {
        rows,
        offered,
        dropped,
    }
}

/// Frames whose slope ratio is **physically impossible** are refused, not averaged.
///
/// `r = dD_c/dD_r` is how fast one channel's density rises against red's. Density rises with
/// exposure in every channel of every film, so `r <= 0` cannot be a property of the stock —
/// it is a frame whose scene defeated the fit. Ektar's `1626` returns `r_b = -0.91` and it is
/// not a rounding effect: including it moves that roll's blue nulling scale from 0.790 to
/// 0.849, which is *larger* than the whole Ektar-vs-Portra blue gap the cross-roll block is
/// asked to adjudicate, and it inflates the `se` that the same block divides by.
fn physically_possible(r: &RollRow) -> bool {
    r.r_g > 0.0 && r.r_b > 0.0
}

/// The headline estimator: mean over frames that are physically possible and within `±2 sd`
/// of that subset's own mean.
///
/// **This is the value `scripts/scale-review/` renders and the progress log quotes**, so it
/// lives here rather than being recomputed by hand — an earlier round derived it out-of-band,
/// the probe was then rewritten without it, and the review set was left citing a gain no
/// committed code produced. Returns the kept rows so callers can report what it dropped.
fn trimmed<'a>(rows: &[&'a RollRow], f: &dyn Fn(&RollRow) -> f32) -> Vec<&'a RollRow> {
    let possible: Vec<&RollRow> = rows
        .iter()
        .copied()
        .filter(|r| physically_possible(r))
        .collect();
    if possible.len() < 4 {
        return possible;
    }
    let n = possible.len() as f32;
    let m = possible.iter().map(|r| f(r)).sum::<f32>() / n;
    let sd = (possible.iter().map(|r| (f(r) - m).powi(2)).sum::<f32>() / (n - 1.0)).sqrt();
    possible
        .into_iter()
        .filter(|r| (f(r) - m).abs() <= 2.0 * sd)
        .collect()
}

/// `(mean, median, sd, se)` of one field over a set of rows — in that order. Two of the
/// four are a factor of sqrt(n) apart, so a caller that binds them wrongly publishes a
/// plausible number rather than failing.
fn stats(rows: &[&RollRow], f: &dyn Fn(&RollRow) -> f32) -> (f32, f32, f32, f32) {
    let n = rows.len() as f32;
    let m = rows.iter().map(|r| f(r)).sum::<f32>() / n;
    let sd = (rows.iter().map(|r| (f(r) - m).powi(2)).sum::<f32>() / (n - 1.0)).sqrt();
    let mut v: Vec<f32> = rows.iter().map(|r| f(r)).collect();
    v.sort_by(f32::total_cmp);
    let med = if v.len().is_multiple_of(2) {
        (v[v.len() / 2 - 1] + v[v.len() / 2]) / 2.0
    } else {
        v[v.len() / 2]
    };
    (m, med, sd, sd / n.sqrt())
}

/// Whole-roll per-channel scale: can **one roll** resolve its own `density.scale`, and is
/// what it resolves a property of the *roll* or of the *scanner*?
///
/// [`sigmoid_scale`]'s constant nulls the corpus *mean*, not any roll — per-roll residuals
/// span ±0.5 stop per density, and `io/scanner-density-calibration` records that resolving a
/// 0.3 stops/density difference needs ~11 frames of one condition, with two earlier per-roll
/// conclusions retracted for reading n = 3–4 too confidently. Every roll in [`FIXTURES`] has
/// 3–5 frames, so a per-roll statement was never supportable. [`WHOLE_ROLLS`] carries two
/// rolls of ~32 frames, one stock each.
///
/// **Two rolls is what separates the two hypotheses.** One roll can show that a fitted gain
/// beats the shipped constant, but not *why*. If the second roll — different stock, same
/// scanner — lands on the same gain, the quantity is a scanner property and a per-roll knob
/// is the wrong shape for it; if it lands elsewhere, it is per-roll. The cross-roll block at
/// the end is that comparison.
///
/// Each roll uses its **own** frozen base (leader + unexposed frame, frozen by the harness
/// with explicit regions because a cropped holder defeats the auto rebate search). The slope
/// does not actually need it — a base error is a per-channel constant and
/// [`measure_decoded`] normalises every bin against the middle one, which was verified by
/// re-running an earlier borrowed-base measurement and getting identical slopes to four
/// decimals — but the level in [`whole_roll_white_point`] does.
///
/// ```text
/// cargo test --release curve_probe::whole_roll_scale -- --ignored --nocapture
/// ```
#[test]
#[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
fn whole_roll_scale() {
    let Some(assets) = assets_root() else {
        eprintln!("SKIP: no ../nc-assets/manifest.json (set NC_ASSETS to override)");
        return;
    };
    let log2 = std::f32::consts::LN_2 / std::f32::consts::LN_10;
    let k = REFERENCE_CONTRAST / log2;
    let drift = |s: f32, r: f32| k * (s * r - 1.0);
    let gm = |sg: f32, sb: f32, r: &RollRow| drift(sg, r.r_g) - drift(sb, r.r_b) / 2.0;

    let mut summary: Vec<(&str, usize, f32, f32, f32, f32)> = vec![];
    for (roll, stock_key) in WHOLE_ROLLS {
        let m = measure_roll(&assets, roll, stock_key);
        let rows = &m.rows;
        if rows.len() < 4 {
            println!("\n=== {roll}: only {} frames measured, skipped", rows.len());
            continue;
        }
        println!("\n=== {roll} ({stock_key}) ===");
        m.report_drops();
        println!(
            "  {:28}{:>9}{:>9}{:>8}{:>8}  used",
            "frame", "r_green", "r_blue", "span", "oor"
        );
        for r in rows {
            let mark = if !physically_possible(r) {
                "refused: r <= 0"
            } else if r.span >= MIN_SPAN {
                "y"
            } else {
                "-"
            };
            println!(
                "  {:28}{:>9.4}{:>9.4}{:>8.2}{:>8.3}  {mark}",
                r.stem, r.r_g, r.r_b, r.span, r.oor
            );
        }
        let kept: Vec<&RollRow> = rows.iter().filter(|r| r.span >= MIN_SPAN).collect();
        if kept.len() < 4 {
            println!(
                "  only {} frames clear the span filter, skipped",
                kept.len()
            );
            continue;
        }
        let (mg, medg, sdg, seg) = stats(&kept, &|r: &RollRow| r.r_g);
        let (mb, medb, sdb, seb) = stats(&kept, &|r: &RollRow| r.r_b);
        println!("\n  span >= {MIN_SPAN} (n = {}):", kept.len());
        println!("    {:16}{:>10}{:>10}", "", "green", "blue");
        println!("    {:16}{mg:>10.4}{mb:>10.4}", "mean r");
        println!("    {:16}{medg:>10.4}{medb:>10.4}", "median r");
        println!("    {:16}{sdg:>10.4}{sdb:>10.4}", "sd");
        println!("    {:16}{seg:>10.4}{seb:>10.4}", "se");
        println!(
            "    {:16}{:>10.3}{:>10.3}   (1/mean, ±1 se: green ±{:.3}, blue ±{:.3})",
            "  from mean",
            1.0 / mg,
            1.0 / mb,
            seg / (mg * mg),
            seb / (mb * mb)
        );
        println!(
            "    {:16}{:>10.3}{:>10.3}   (1/median)",
            "  from median",
            1.0 / medg,
            1.0 / medb
        );

        // The headline. Everything downstream — the cross-roll comparison and the review
        // set's `sig-roll` gain — uses this, not the plain mean: one physically impossible
        // frame moves the plain mean further than the between-roll gap it is asked to judge.
        let tg = trimmed(&kept, &|r: &RollRow| r.r_g);
        let tb = trimmed(&kept, &|r: &RollRow| r.r_b);
        let (tmg, _, _, tseg) = stats(&tg, &|r: &RollRow| r.r_g);
        let (tmb, _, _, tseb) = stats(&tb, &|r: &RollRow| r.r_b);
        println!(
            "    {:16}{:>10.3}{:>10.3}   <- HEADLINE: physically possible, +/-2 sd\n    \
             {:16}{:>10}{:>10}   (+/-1 se: green +/-{:.3}, blue +/-{:.3})",
            "  trimmed",
            1.0 / tmg,
            1.0 / tmb,
            "",
            format!("n={}", tg.len()),
            format!("n={}", tb.len()),
            tseg / (tmg * tmg),
            tseb / (tmb * tmb)
        );

        // Correlated channels are one observation, not two — and a near-equal green and blue
        // against red is the scan-path signature the datasheets do not predict.
        let c = kept.len() as f32;
        let (cg, cb) = (mg, mb);
        let num: f32 = kept.iter().map(|r| (r.r_g - cg) * (r.r_b - cb)).sum();
        let den = (kept.iter().map(|r| (r.r_g - cg).powi(2)).sum::<f32>()
            * kept.iter().map(|r| (r.r_b - cb).powi(2)).sum::<f32>())
        .sqrt();
        println!("    r_green vs r_blue correlation: {:+.3}", num / den);

        // The characteristic path on the same frames: what each stock's own published
        // tables leave behind, against what the scalar path leaves. This is the comparison
        // that says whether the curve fixes a channel, and it is per stock.
        let (cg, _, csg, ceg) = stats(&kept, &|r: &RollRow| r.curve_g);
        let (cb, _, csb, ceb) = stats(&kept, &|r: &RollRow| r.curve_b);
        let scalar_g = k * (mg - 1.0);
        let scalar_b = k * (mb - 1.0);
        println!("\n    residual drift, stops per unit density (0 = neutral):");
        println!("      {:22}{:>10}{:>10}", "path", "green", "blue");
        println!(
            "      {:22}{scalar_g:>+10.2}{scalar_b:>+10.2}",
            "scalar (identity gain)"
        );
        println!(
            "      {:22}{cg:>+10.2}{cb:>+10.2}   (sd {csg:.2}/{csb:.2}, se {ceg:.2}/{ceb:.2})",
            "characteristic"
        );
        // A "% removed" against a baseline that is already neutral is meaningless — and
        // worse, a curve that pushes a neutral channel off neutral prints a large negative
        // percentage that reads like a rounding artefact rather than the over-correction it
        // is. Say which it is instead.
        let removed = |scalar: f32, resid: f32| -> String {
            if scalar.abs() < 0.25 {
                format!("n/a (was {scalar:+.2})")
            } else if resid.abs() > scalar.abs() {
                format!("WORSE {:+.2}", resid - scalar)
            } else {
                format!("{:.0}%", 100.0 * (1.0 - resid.abs() / scalar.abs()))
            }
        };
        println!(
            "      {:22}{:>10}{:>18}",
            "removed by the curve",
            removed(scalar_g, cg),
            removed(scalar_b, cb)
        );

        // Split-half: the roll's own null is fitted on the frames it is scored against, so
        // its in-sample number is circular. Fit on one half, score on the other.
        let a: Vec<&RollRow> = kept.iter().step_by(2).copied().collect();
        let b: Vec<&RollRow> = kept.iter().skip(1).step_by(2).copied().collect();
        let score = |sg: f32, sb: f32, set: &[&RollRow]| {
            set.iter().map(|r| gm(sg, sb, r).abs()).sum::<f32>() / set.len() as f32
        };
        let (mut fit_sum, mut def_sum) = (0.0, 0.0);
        for (fit_on, held) in [(&a, &b), (&b, &a)] {
            // Fit on the same estimator the headline uses, or the fold containing the
            // impossible frame fits to it and the held-out score reports that, not the roll.
            let fg = trimmed(fit_on, &|r: &RollRow| r.r_g);
            let fb = trimmed(fit_on, &|r: &RollRow| r.r_b);
            if fg.is_empty() || fb.is_empty() {
                continue;
            }
            let sg = fg.len() as f32 / fg.iter().map(|r| r.r_g).sum::<f32>();
            let sb = fb.len() as f32 / fb.iter().map(|r| r.r_b).sum::<f32>();
            fit_sum += score(sg, sb, held);
            def_sum += score(0.90, 0.86, held);
        }
        println!(
            "    split-half |green-magenta|, held out: fitted {:.2} vs current default {:.2}",
            fit_sum / 2.0,
            def_sum / 2.0
        );
        println!(
            "    green-magenta at the current default [1, 0.90, 0.86]: {:+.2}",
            kept.iter().map(|r| gm(0.90, 0.86, r)).sum::<f32>() / c
        );
        summary.push((
            roll,
            tg.len(),
            tmg,
            tmb,
            tseg / (tmg * tmg),
            tseb / (tmb * tmb),
        ));
    }

    println!("\n=== cross-roll: is the fitted gain a roll property or a scanner property? ===");
    println!(
        "  {:24}{:>6}{:>10}{:>10}{:>12}{:>12}",
        "roll", "n", "mean r_g", "mean r_b", "scale green", "scale blue"
    );
    for (roll, n, mg, mb, eg, eb) in &summary {
        println!(
            "  {roll:24}{n:>6}{mg:>10.4}{mb:>10.4}{:>9.3}±{eg:.3}{:>9.3}±{eb:.3}",
            1.0 / mg,
            1.0 / mb
        );
    }
    println!(
        "  {:24}{:>6}{:>10.4}{:>10.4}{:>12.3}{:>12.3}",
        "shipped default",
        "",
        1.0 / 0.90,
        1.0 / 0.86,
        0.900,
        0.860
    );
    // Every pair, named. This block is the probe's only cross-roll statement and both the
    // task file and the progress log tell the next person to "add a roll to `WHOLE_ROLLS` and
    // re-run" — hard-indexing `summary[0]`/`[1]` would leave a third roll out of the verdict
    // silently, at exit 0, while the sentence still said "the two rolls".
    if summary.len() >= 2 {
        println!("\n  separation between rolls, in combined se:");
        for i in 0..summary.len() {
            for j in (i + 1)..summary.len() {
                let (ri, _, gi, bi, egi, ebi) = summary[i];
                let (rj, _, gj, bj, egj, ebj) = summary[j];
                let sg = (1.0 / gi - 1.0 / gj).abs() / (egi * egi + egj * egj).sqrt();
                let sb = (1.0 / bi - 1.0 / bj).abs() / (ebi * ebi + ebj * ebj).sqrt();
                println!("    {ri} vs {rj}: green {sg:.1} se, blue {sb:.1} se");
            }
        }
        println!(
            "  Near zero on every pair => the rolls want one gain, so it is a property of the\n  \
             scan path and a per-ROLL knob is the wrong shape. Large => per-roll is real.\n  \
             {} roll(s) contributed; a pair is only as good as the variety behind it — rolls\n  \
             sharing a photographer and a palette agree for that reason alone.",
            summary.len()
        );
    }
}

/// Is a roll-wide **white point** a usable source for the per-channel *offset*?
///
/// The premise under test (user, 2026-09-10): however badly a roll was exposed, its brightest
/// content is probably white, so a roll-aggregated near-white could supply the per-channel
/// level correction that [`whole_roll_scale`]'s gain structurally cannot. Three things bound
/// what this can show:
///
/// - **A white point is an offset, never a slope.** One point fixes a level; separating level
///   from slope needs two at a known separation. So this measures the half `density.scale`
///   leaves untouched — and, unlike the scale, it is *not* immune to a wrong film base.
/// - **Without a known neutral it cannot be validated, only checked for self-consistency.**
///   "The brightest thing was white" and "the blue layer runs hot" are indistinguishable from
///   scene content alone — the gap `io/scanner-density-calibration` wants a ColorChecker for.
///   What *is* decidable is whether a roll has a consistent white point at all: if per-frame
///   estimates scatter widely, a roll aggregate is averaging scene colour rather than
///   measuring the film, and the idea fails on its own terms.
/// - **Most of the difference may already be carried by the slope.** At red density `w_r`, a
///   channel running `r` times steeper sits `(r − 1)·w_r` above red for that reason alone.
///   Only the residual is new information, and correcting the whole difference with an offset
///   would double-correct the half `density.scale` already handles. That split is the last
///   block; for the figures it currently produces see the 2026-09-13 entry in
///   `docs/progress/film-base.md` (it supersedes 2026-09-12's numbers) — quoting them here is how this doc came to cite a
///   pre-span-filter 85%/34% that the shipped probe had stopped producing.
///
/// Grey-world is measured on the same frames as a control: the two estimators lean on
/// opposite assumptions, so agreement is weak evidence the signal is the film and
/// disagreement proves at least one is reading the scene.
///
/// ```text
/// cargo test --release curve_probe::whole_roll_white_point -- --ignored --nocapture
/// ```
#[test]
#[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
fn whole_roll_white_point() {
    let Some(assets) = assets_root() else {
        eprintln!("SKIP: no ../nc-assets/manifest.json (set NC_ASSETS to override)");
        return;
    };
    let log2 = std::f32::consts::LN_2 / std::f32::consts::LN_10;
    let stops = REFERENCE_CONTRAST / log2;

    for (roll, stock_key) in WHOLE_ROLLS {
        let m = measure_roll(&assets, roll, stock_key);
        let rows = &m.rows;
        // The span filter matters here too, and not only for conditioning: a uniform field
        // has no "near-white" to speak of, and a calibration frame carried as `real` would
        // otherwise enter the level statistics.
        let kept: Vec<&RollRow> = rows.iter().filter(|r| r.span >= MIN_SPAN).collect();
        if kept.len() < 4 {
            println!("\n=== {roll}: only {} usable frames, skipped", kept.len());
            continue;
        }
        println!("\n=== {roll} ({stock_key}): white point ===");
        m.report_drops();
        println!(
            "  {} of {} measured frames clear the span filter",
            kept.len(),
            rows.len()
        );
        println!(
            "  Per-channel density relative to red. near-white = p{NEAR_WHITE}, grey = frame mean.\n"
        );
        println!(
            "  {:16}{:>10}{:>10}{:>10}{:>12}",
            "", "mean", "sd", "se", "mean stops"
        );
        // Computed here, not recovered from the print loop by matching a display label: a
        // renamed or re-spaced label would otherwise leave these at an initialiser, and the
        // slope/offset block below divides by `measured`, so the probe would print `inf` and
        // `NaN` at exit 0 instead of failing.
        let wg = stats(&kept, &|r: &RollRow| r.w_g);
        let wb = stats(&kept, &|r: &RollRow| r.w_b);
        for (label, (m, _med, sd, se)) in [
            ("white  g-r", wg),
            ("white  b-r", wb),
            ("grey   g-r", stats(&kept, &|r: &RollRow| r.g_g)),
            ("grey   b-r", stats(&kept, &|r: &RollRow| r.g_b)),
        ] {
            println!(
                "  {label:16}{m:>10.4}{sd:>10.4}{se:>10.4}{:>12.2}",
                m * stops
            );
        }

        // Does one roll-level white point predict a frame it was not fitted on? Scored as RMS
        // distance to each held-out frame's own estimate, against the "no offset" baseline.
        let a: Vec<&RollRow> = kept.iter().step_by(2).copied().collect();
        let b: Vec<&RollRow> = kept.iter().skip(1).step_by(2).copied().collect();
        let rms = |set: &[&RollRow], f: &dyn Fn(&RollRow) -> f32, c: f32| {
            (set.iter().map(|r| (f(r) - c).powi(2)).sum::<f32>() / set.len() as f32).sqrt()
        };
        println!("\n  split-half: does a roll-level white point predict a held-out frame?");
        println!(
            "    {:14}{:>16}{:>16}",
            "channel", "held-out RMS", "no-offset RMS"
        );
        for (label, f) in [
            (
                "white  g-r",
                &(|r: &RollRow| r.w_g) as &dyn Fn(&RollRow) -> f32,
            ),
            ("white  b-r", &(|r: &RollRow| r.w_b)),
        ] {
            let (mut fit, mut zero) = (0.0, 0.0);
            for (fit_on, held) in [(&a, &b), (&b, &a)] {
                let c = fit_on.iter().map(|r| f(r)).sum::<f32>() / fit_on.len() as f32;
                fit += rms(held, f, c);
                zero += rms(held, f, 0.0);
            }
            println!("    {label:14}{:>16.4}{:>16.4}", fit / 2.0, zero / 2.0);
        }

        // Slope or offset? Only the residual is information a gain does not already carry.
        let n = kept.len() as f32;
        let mean_w_r = kept.iter().map(|r| r.w_r).sum::<f32>() / n;
        // Same trimmed estimator the scale half reports: a plain mean here would carry the
        // physically impossible frame straight into the slope/offset split, which is the
        // load-bearing number of this probe.
        let tg = trimmed(&kept, &|r: &RollRow| r.r_g);
        let tb = trimmed(&kept, &|r: &RollRow| r.r_b);
        let r_g = tg.iter().map(|r| r.r_g).sum::<f32>() / tg.len() as f32;
        let r_b = tb.iter().map(|r| r.r_b).sum::<f32>() / tb.len() as f32;
        println!(
            "  (slope from the trimmed estimator: green n={}, blue n={})",
            tg.len(),
            tb.len()
        );
        println!("\n  is it slope or offset? mean red density at near-white = {mean_w_r:.4}");
        println!(
            "    {:12}{:>12}{:>14}{:>12}{:>10}{:>10}",
            "channel", "measured", "slope part", "residual", "stops", "% slope"
        );
        for (label, measured, r) in [("green g-r", wg.0, r_g), ("blue  b-r", wb.0, r_b)] {
            let slope_part = (r - 1.0) * mean_w_r;
            let resid = measured - slope_part;
            println!(
                "    {label:12}{measured:>12.4}{slope_part:>14.4}{resid:>12.4}{:>10.2}{:>9.0}%",
                resid * stops,
                100.0 * slope_part / measured
            );
        }
    }
    println!(
        "\n  Read sd against mean. A roll white point is a film property only if the per-frame\n  \
         estimates cluster; if sd is the size of the mean, the aggregate is scene colour\n  \
         averaged, and no roll-scoping rescues it. And read `% slope` before acting on any\n  \
         residual: that share is already carried by `density.scale`."
    );
}
