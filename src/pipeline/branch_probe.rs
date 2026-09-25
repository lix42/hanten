//! Test-only probe for `nf-display-stages/branch-contract`: renders real frames as an
//! SDR/HDR pair through the new chain and prints derived numbers only — how the pair
//! sits against the branch contract below diffuse white, how far the HDR rendition
//! overshoots its peak (the open question of whether an HDR destination needs a hard
//! ceiling), and the gain map the pair makes.
//!
//! `#[ignore]`d and asset-gated, like `pipeline::shadow_metrics`: it skips with a
//! message when `../nc-assets` is absent, and never prints a pixel.
//!
//! ```text
//! cargo test --release branch_probe -- --ignored --nocapture
//! ```
//!
//! Every picture frame (manifest role `real`) of each roll in [`ROLLS`] is decoded
//! with the default recipe and the roll's base, subsampled on both axes (the decode is
//! per-pixel, so this samples the frame rather than changing the render), and rendered
//! by `chain::render_pair`. Three environment variables vary the run:
//! `NC_PROBE_STRIDE` (default 2), `NC_PROBE_HEADROOM` (stops; default the recipe's)
//! and `NC_PROBE_HDR_PEAK` (times diffuse white; default 1000/203).

use std::path::{Path, PathBuf};

use crate::algo::fixed;
use crate::pipeline::chain::{self, contract};
use crate::pipeline::fit_gamut::DestinationGamut;
use crate::pipeline::fit_range::DisplayPeak;
use crate::pipeline::gain_ratio;
use crate::pipeline::working_space::{AcesCgImage, map_nc_film_rgb_v1};
use crate::recipe::Recipe;
use crate::types::{FilmBase, LinearImage};

/// The rolls measured and each one's film base: `2026-09-09-Ektar100` from its frozen
/// recipe (`scripts/real-scan-verify/recipes/2026-09-09-Ektar.json`), the other three
/// from `hanten estimate --grid` on the roll's `base.tif` (the bases
/// `nf-look/desaturation-band-fit` used). The four rolls `fit-gamut` measured.
const ROLLS: &[(&str, [f32; 3])] = &[
    (
        "2026-09-09-Ektar100",
        [0.391_622_8, 0.200_274_66, 0.130_846_11],
    ),
    (
        "2026-09-11-Portra400",
        [0.346_913_87, 0.162_584_87, 0.093_369_95],
    ),
    (
        "2026-09-18-Gold200",
        [0.470_954_45, 0.232_440_68, 0.108_033_87],
    ),
    (
        "2026-09-20-Portra400",
        [0.420_981_17, 0.199_328_6, 0.109_697_11],
    ),
];

/// An `f64` from the environment, or `default`; a value that does not parse panics
/// naming the variable.
fn env_or(name: &str, default: f64) -> f64 {
    std::env::var(name).map_or(default, |v| {
        v.parse()
            .unwrap_or_else(|e| panic!("{name}={v:?} is not a number: {e}"))
    })
}

/// Decode budget, matching the shipped default (see `shadow_metrics`).
const DECODE_BUDGET_BYTES: u64 = 6 * 1024 * 1024 * 1024;

/// The format policy the current chain's gain map uses (`gain_map`'s Ultra HDR v1
/// offset), restated rather than imported so the probe does not follow it.
const OFFSET: f32 = 1.0 / 64.0;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn assets_root() -> Option<PathBuf> {
    let root = std::env::var("NC_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("../nc-assets"));
    root.join("manifest.json").is_file().then_some(root)
}

fn real_frames(assets: &Path, roll: &str) -> Vec<PathBuf> {
    let text = std::fs::read_to_string(assets.join("manifest.json")).unwrap();
    let m: serde_json::Value = serde_json::from_str(&text).unwrap();
    let Some(frames) = m["rolls"][roll]["frames"].as_array() else {
        panic!("roll {roll} is not in the manifest");
    };
    let mut out: Vec<PathBuf> = frames
        .iter()
        .filter(|f| f["role"].as_str().unwrap_or("real") == "real")
        .map(|f| assets.join(f["file"].as_str().unwrap()))
        .collect();
    out.sort();
    out
}

/// Every `stride`-th pixel on both axes, without the IR plane.
fn subsample(image: &LinearImage, stride: u32) -> LinearImage {
    let (w, h) = (image.width.div_ceil(stride), image.height.div_ceil(stride));
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in (0..image.height).step_by(stride as usize) {
        for x in (0..image.width).step_by(stride as usize) {
            let i = ((y * image.width + x) * 3) as usize;
            rgb.extend_from_slice(&image.rgb[i..i + 3]);
        }
    }
    LinearImage::new(w, h, rgb, None).unwrap()
}

/// Totals over a set of frames.
#[derive(Default)]
struct Tally {
    frames: usize,
    pixels: usize,
    below_white: usize,
    identical: usize,
    sdr_bound: usize,
    both_bound: usize,
    violations: usize,
    hdr_samples_over_peak: usize,
    hdr_frames_over_peak: usize,
    hdr_max_over_peak: f32,
    sdr_samples_over_one: usize,
    flat_maps: usize,
    worst_rebuild: f32,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        self.frames += other.frames;
        self.pixels += other.pixels;
        self.below_white += other.below_white;
        self.identical += other.identical;
        self.sdr_bound += other.sdr_bound;
        self.both_bound += other.both_bound;
        self.violations += other.violations;
        self.hdr_samples_over_peak += other.hdr_samples_over_peak;
        self.hdr_frames_over_peak += other.hdr_frames_over_peak;
        self.hdr_max_over_peak = self.hdr_max_over_peak.max(other.hdr_max_over_peak);
        self.sdr_samples_over_one += other.sdr_samples_over_one;
        self.flat_maps += other.flat_maps;
        self.worst_rebuild = self.worst_rebuild.max(other.worst_rebuild);
    }

    fn print(&self, name: &str) {
        let pct = |n: usize, d: usize| 100.0 * n as f64 / d.max(1) as f64;
        let samples = 3 * self.pixels;
        println!(
            "{name}\tframes {}\tbelow-white {:.2}%\tidentical {:.4}%\tsdr-bound {:.4}%\tboth-bound {}\t\
             violations {}\thdr>peak {} ({:.4}% of samples, {} frames, max {:.4}·P)\t\
             sdr>1 {:.4}%\tflat maps {}\tworst rebuild {:.2e}",
            self.frames,
            pct(self.below_white, self.pixels),
            pct(self.identical, self.below_white),
            pct(self.sdr_bound, self.below_white),
            self.both_bound,
            self.violations,
            self.hdr_samples_over_peak,
            pct(self.hdr_samples_over_peak, samples),
            self.hdr_frames_over_peak,
            self.hdr_max_over_peak,
            pct(self.sdr_samples_over_one, samples),
            self.flat_maps,
            self.worst_rebuild,
        );
    }
}

fn measure(image: &LinearImage, base: &FilmBase, recipe: &Recipe, peak: DisplayPeak) -> Tally {
    let aces = || -> AcesCgImage {
        map_nc_film_rgb_v1(
            fixed::decode(image, base, &recipe.reconstruction)
                .unwrap()
                .0,
        )
    };
    let shared = recipe.shared_params();
    let graded = contract::graded(aces(), &shared);
    let pair = chain::render_pair(aces(), &shared, DestinationGamut::DisplayP3, peak).unwrap();
    let (sdr, _) = pair.sdr.image.into_parts();
    let (hdr, _) = pair.hdr.image.into_parts();
    let agreement = contract::check(
        &graded,
        &sdr.rgb,
        &hdr.rgb,
        DestinationGamut::DisplayP3,
        peak,
    );

    let p = peak.value();
    let over: Vec<f32> = hdr.rgb.iter().copied().filter(|v| *v > p).collect();
    let gains = gain_ratio::between(&sdr, &hdr, OFFSET).unwrap();
    let rebuilt = gains.apply_to(&sdr).unwrap();
    let worst_rebuild = rebuilt
        .iter()
        .zip(&hdr.rgb)
        .map(|(got, want)| (got - want).abs() / want.max(1.0))
        .fold(0.0f32, f32::max);
    Tally {
        frames: 1,
        pixels: (sdr.width * sdr.height) as usize,
        below_white: agreement.below_white,
        identical: agreement.identical,
        sdr_bound: agreement.sdr_bound,
        both_bound: agreement.both_bound,
        violations: agreement.violations.len(),
        hdr_samples_over_peak: over.len(),
        hdr_frames_over_peak: usize::from(!over.is_empty()),
        hdr_max_over_peak: over.iter().fold(0.0f32, |m, v| m.max(v / p)),
        sdr_samples_over_one: sdr.rgb.iter().filter(|v| **v > 1.0).count(),
        flat_maps: usize::from(gains.range().flat),
        worst_rebuild,
    }
}

#[test]
#[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
fn branch_probe() {
    let Some(assets) = assets_root() else {
        eprintln!("SKIP: no ../nc-assets/manifest.json (set NC_ASSETS to override)");
        return;
    };
    let stride = env_or("NC_PROBE_STRIDE", 2.0);
    assert!(
        stride >= 1.0 && stride.fract() == 0.0,
        "NC_PROBE_STRIDE must be a positive whole number, got {stride}"
    );
    let stride = stride as u32;
    let peak = DisplayPeak::new(env_or("NC_PROBE_HDR_PEAK", 1000.0 / 203.0) as f32).unwrap();
    let mut recipe = Recipe::default();
    recipe.fit_range.headroom_stops = env_or(
        "NC_PROBE_HEADROOM",
        f64::from(recipe.fit_range.headroom_stops),
    ) as f32;
    println!(
        "stride {stride}, HDR peak {:.4}, headroom {} stops, default recipe",
        peak.value(),
        recipe.fit_range.headroom_stops
    );

    let mut total = Tally::default();
    for &(roll, base) in ROLLS {
        let base = FilmBase::from(base);
        let mut roll_tally = Tally::default();
        for frame in real_frames(&assets, roll) {
            let (image, _) = crate::io::decode::decode_within(&frame, DECODE_BUDGET_BYTES)
                .unwrap_or_else(|e| panic!("{}: {e}", frame.display()));
            let tally = measure(&subsample(&image, stride), &base, &recipe, peak);
            tally.print(&format!(
                "{roll}/{}",
                frame.file_stem().unwrap().to_string_lossy()
            ));
            roll_tally.add(&tally);
        }
        roll_tally.print(&format!("ROLL {roll}"));
        total.add(&roll_tally);
    }
    total.print("TOTAL");
    // Fail loudly rather than pass on nothing: a renamed roll yields no frames.
    assert!(total.frames > 0, "no frames measured");
    assert_eq!(
        total.violations, 0,
        "the branch contract is broken on real frames"
    );
    assert_eq!(
        total.both_bound, 0,
        "a below-white difference needed the loose rule for a pixel both cubes bind"
    );
}
