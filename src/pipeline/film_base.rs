//! `Dmin` / film-base measurement (pure).
//!
//! The film base is the unexposed film: minimum density, hence **maximum
//! transmission**. Its per-channel transmission is the divisor of the density
//! conversion (`D = -log10(scan / Dmin)`), so it sets black point and colour balance
//! together.
//!
//! A base is measured from an **area** and a **method**, and nothing else
//! (`film-base/holder-masked-measurement`):
//!
//! - **The effective area** of a reference frame ([`effective_area`]: the IR-measured
//!   holder cut, then a static inset), read by [`measure_area`] at the per-channel
//!   **median**. With the holder cut away the area is one population — unexposed film
//!   plus grain and scanner noise — and an extreme percentile would land in its noise
//!   tail: p97 sat 0.046 density above the median on a Gold 200 leader, ~0.16 stops
//!   pale. It measures an *unexposed* frame; over a picture it returns a plausible,
//!   wrong base, which its uniformity warning exists to catch.
//! - **A region the user states** ([`FilmBaseSource::Region`]), read at
//!   [`SAMPLE_PERCENTILE`] (p97). A hand-drawn rectangle may still mix holder, rebate
//!   and picture, and there the base is the most transparent sub-population, which a
//!   high percentile reaches past the rest.
//!
//! Nothing searches for a rebate: the inset passes over it blind. The effective-area
//! measurement is not a [`FilmBaseSource`] — it runs only in the measurement commands,
//! and a conversion takes their result as an explicit base.
//!
//! The effective area also feeds [`polarity_warning`] on every `convert`, `roll` and
//! `measure-roll` picture frame, so a change to the area or inset moves that warning
//! and a `--strict` exit.

use serde::Serialize;

use crate::types::{FilmBase, FilmBaseSource, LinearImage, NcError, Result, check_measure_inset};

/// The percentile a stated region is read at. High, because a region may be a
/// mixture whose most transparent sub-population is the base; below the maximum, so a
/// hot pixel cannot become it.
const SAMPLE_PERCENTILE: f32 = 0.97;

/// Low percentile paired with [`SAMPLE_PERCENTILE`] for the region's uniformity check.
const LOW_PERCENTILE: f32 = 0.10;

/// Max acceptable per-channel relative spread `(p97 - p10) / p97` for a stated region
/// to count as uniform unexposed film. Applied to every channel.
const MAX_RELATIVE_SPREAD: f32 = 0.15;

/// The percentile [`measure_area`] reads: the median. Over one population the
/// estimator's precision is irrelevant at millions of pixels, so it is chosen for
/// contamination, which on an unexposed frame is one-sided (dust and a holder sliver
/// darker, a light leak brighter) — and the median holds until half the area is bad.
pub const AREA_PERCENTILE: f32 = 0.5;

/// The percentiles bounding [`measure_area`]'s uniformity spread
/// `(p90 - p10) / p50`: symmetric about the estimate, so the spread describes the
/// population the median was read from.
const AREA_SPREAD_PERCENTILES: [f32; 2] = [0.10, 0.90];

/// Worst per-channel [`AREA_SPREAD_PERCENTILES`] spread above which the effective area
/// is reported as not uniform — the sign that the frame is not unexposed film.
///
/// Measured 2026-09-28 over `../nc-assets`: the 9 unexposed frames read 0.06-0.29
/// (grain and scanner noise, wider on the thinner-scanned 2026-09 rolls; each
/// frame's centre and whole-area medians agree to <1%, so it is not a gradient),
/// picture frames 0.87-2.26. This is their geometric midpoint. It is a coarse "is
/// this a picture?" guard, not a uniformity verdict: a leader (0.23-0.45) and a
/// near-blank frame pass it.
pub const AREA_MAX_RELATIVE_SPREAD: f32 = 0.5;

/// The 16-bit code count: decoded samples are `code / 65535`
/// (`io::decode::normalize_u16`), so a histogram over the codes holds every sample
/// exactly.
const CODES: usize = 1 << 16;

/// IR transmission at or below which a near-edge segment reads as the opaque film
/// holder. Chromogenic film (base, rebate, picture, even fully-exposed leader) is
/// IR-transparent and reads bright — measured ≈ 0.6–0.7 on real HDRi scans — while
/// the holder blocks IR and reads dark (≈ 0.02), a ~25× separation, so 0.1 splits
/// them with wide margin on both sides (`ir-holder-detection` verification data:
/// Phoenix holder IR 0.023, Ektar fully-exposed film IR 0.587).
const IR_HOLDER_MAX_TRANSMISSION: f32 = 0.1;

/// Interior IR transmission below which this frame's own **film** is too opaque
/// for the holder classifier above to mean anything — the IR usability verdict
/// (`ir-usability-detection`). Silver-halide film blocks IR in proportion to
/// accumulated density, so usability is a property of the *frame*, not of the
/// stock's chemistry: an unexposed silver frame is IR-transparent against an
/// opaque holder while its own leader is opaque throughout.
///
/// `2.5x` [`IR_HOLDER_MAX_TRANSMISSION`] by construction — film reading at the
/// level that *means* holder cannot be told from holder, so a usable frame must
/// clear that threshold with margin. Measured 2026-09-04 over `../nc-assets`
/// (IR planes read directly; derived numbers only): 25 chromogenic frames across
/// 9 rolls, every role including 8 leaders, span **0.576-0.728** — 2.3x above
/// this line, with dye staying IR-transparent at any exposure. The Ilford HP5
/// (silver) roll spans 0.0165 (leader) to 0.4730 (unexposed), and this line
/// reproduces every verdict `docs/tasks/film-base/ir-usability-detection.md`
/// records.
///
/// The tightest real margin is frame 1335 at 0.2711 (1.08x above), a half-clear
/// half-dense frame — and a wrong verdict there errs in the safe direction: its
/// dense film reads holder and the march over-cuts, which costs measurement area
/// rather than admitting holder.
/// What would move this constant: a chromogenic frame measuring below ~0.4 (none
/// of 25 does), or evidence that separability on silver is decided by something
/// other than the frame's own density.
const IR_USABLE_MIN_INTERIOR: f32 = 2.5 * IR_HOLDER_MAX_TRANSMISSION;

/// Fraction of the short dimension trimmed off each side before the usability
/// verdict samples the interior: the holder occludes the frame *edge*, so a
/// verdict about the film must not read the border it exists to detect.
const IR_USABLE_INTERIOR_MARGIN: f32 = 0.10;

/// Target sample count for the usability verdict — a bound on the *work*, not an
/// exact size: both axes round their stride up independently, so an elongated
/// interior lands somewhat above it. It strides the interior instead of
/// materializing it, so the check costs a few hundred KB at any frame size
/// (10368x7200 samples 98,774 values) and `pipeline/memory.rs` owes it no term.
const IR_USABLE_MAX_SAMPLES: usize = 100_000;

/// Number of along-edge segments the holder march splits each edge into. A holder
/// can occlude only *part* of an edge (e.g. Phoenix `933` right), so a single
/// per-edge median is too coarse; ~24 segments resolve a partial holder while each
/// still pools many pixels.
const IR_HOLDER_SEGMENTS: u32 = 24;

/// Thickness of one band of the holder march, as a fraction of the short dimension:
/// its step, and so its resolution. Shallow, because a deep band dilutes a thin
/// holder with the bright film behind it; on real HDRi scans (`ir-holder-detection`)
/// a ~0.5% band reads Phoenix `933` top/right as holder (IR ≈ 0.02) and bottom/left
/// as film (≈ 0.65). Floored at [`IR_HOLDER_PROBE_MIN`] for tiny frames.
const IR_HOLDER_PROBE_FRAC: f32 = 0.005;

/// Minimum holder probe depth in pixels — floors [`IR_HOLDER_PROBE_FRAC`] so a
/// small image still samples more than a single noisy row.
const IR_HOLDER_PROBE_MIN: u32 = 2;

/// How deep [`holder_depths`] marches before giving up on an edge, as a fraction
/// of the shorter dimension. Also clamped to half the perpendicular extent per edge,
/// so two opposing capped edges still leave a region.
///
/// **Premise: no film holder is this deep.** Measured depths are 2.5-4% of the
/// shorter edge (31 real IR frames, 7 rolls). So an edge that reaches the cap read
/// something other than holder — IR-dark film or debris — and its flag in
/// [`CappedEdges`] marks the frame's depths as not a measurement. The error then
/// runs one way: a cap cuts film, never leaves holder, and the perpendicular edges
/// it inflates (they are trimmed by the capped depth, see [`holder_depths`]) cut
/// more film. The region shrinks but stays clean. A holder that really is deeper is
/// an edge case the user covers with the inset (`measure.inset`), which is added
/// on top of the cap.
///
/// Not raised, because the cap is also the only bound on an ambiguous IR read, and
/// a larger one would bring `2 * (cap + inset) >= 1` — the empty region — within
/// reach of ordinary inset values (`film-base/holder-cap-contamination`).
const HOLDER_MARCH_MAX_FRAC: f32 = 0.25;

/// How many times [`holder_depths`] re-measures with the previous pass's depths as
/// the next pass's along-edge trims. It exits early on the first repeat, so raising
/// this cannot change an answer that settled — and on every frame measured so far
/// it settles in two or three passes. Settling is *not* guaranteed, though (the
/// depth map is not monotone; see [`holder_depths`]), so this is a stop, and
/// `HolderDepths::converged` reports which kind of answer came back.
const HOLDER_MARCH_PASSES: usize = 4;

/// A resolved film base plus any non-fatal quality warnings the measurement raised
/// (a non-uniform region or area). The orchestrator folds the warnings into the JSON
/// report, where `--strict` promotes them — the value itself is never altered.
#[derive(Clone, Debug, PartialEq)]
pub struct BaseEstimate {
    pub base: FilmBase,
    pub warnings: Vec<String>,
    /// The per-channel percentile the base was read at — [`AREA_PERCENTILE`] over
    /// the effective area, [`SAMPLE_PERCENTILE`] over a stated region — or `None`
    /// for an explicit base, which reads no pixels. Returned so the report states
    /// the method rather than re-deriving it from the source.
    pub percentile: Option<f32>,
    /// The worst per-channel [`AREA_SPREAD_PERCENTILES`] spread over the effective
    /// area; `None` for a stated source. Detecting an unexposed frame reads it.
    pub spread: Option<f32>,
}

/// An image edge, for the holder march.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// Resolve a **stated** [`FilmBaseSource`] to a base: the explicit value, or the
/// stated region read at [`SAMPLE_PERCENTILE`]. Region bounds are checked here,
/// since the CLI cannot see the image.
///
/// Whatever the source, the base is guaranteed **finite and positive on every
/// channel** ([`guard_base`]) — a zero, negative or non-finite divisor errors here
/// rather than poisoning the render.
pub fn estimate(image: &LinearImage, source: &FilmBaseSource) -> Result<BaseEstimate> {
    let est = match *source {
        FilmBaseSource::Explicit(rgb) => BaseEstimate {
            base: FilmBase::from(rgb),
            warnings: Vec::new(),
            percentile: None,
            spread: None,
        },
        FilmBaseSource::Region(rect) => sample_region(image, rect)?,
    };
    guard_base(&est.base, source_advice(source))?;
    Ok(est)
}

/// Measure the base over a resolved [`EffectiveArea`]: the per-channel
/// [`AREA_PERCENTILE`] (the median) over `area.region`, and a warning when the
/// area's [`AREA_SPREAD_PERCENTILES`] spread exceeds [`AREA_MAX_RELATIVE_SPREAD`] on
/// any channel — the sign that the frame is not unexposed film.
///
/// Takes the area rather than resolving it, so the caller reports the same area the
/// base was read over. Reads a per-channel histogram of the 16-bit codes
/// ([`CODES`]), not a copy of the pixels: exact on decoded data, and a fixed
/// ~1.5 MB whatever the frame size, so `pipeline::memory` owes it no term.
pub fn measure_area(image: &LinearImage, area: &EffectiveArea) -> Result<BaseEstimate> {
    let hist = CodeHistogram::of(image, area.region)?;
    let [lo, hi] = AREA_SPREAD_PERCENTILES;
    let mut base = [0.0f32; 3];
    let mut spread = 0.0f32;
    for (c, b) in base.iter_mut().enumerate() {
        *b = hist.percentile(c, AREA_PERCENTILE);
        let range = hist.percentile(c, hi) - hist.percentile(c, lo);
        spread = spread.max(if *b > 0.0 { range / *b } else { 1.0 });
    }
    let mut warnings = Vec::new();
    if spread > AREA_MAX_RELATIVE_SPREAD {
        warnings.push(format!(
            "the effective area is not uniform (worst per-channel spread (p90 - p10) / \
             p50 = {spread:.2} > {AREA_MAX_RELATIVE_SPREAD:.2}): it does not look like \
             unexposed film, so the median over it is not a film base. Measure an \
             unexposed frame, or state a region of unexposed film (--base-region)"
        ));
    }
    let base = FilmBase::from(base);
    guard_base(
        &base,
        "the effective area has no usable signal on some channel — is this an \
         unexposed frame? Measure one, or state a region of unexposed film \
         (--base-region)",
    )?;
    Ok(BaseEstimate {
        base,
        warnings,
        percentile: Some(AREA_PERCENTILE),
        spread: Some(spread),
    })
}

/// How many times the base a sample must transmit to count against the frame being a
/// negative, in [`polarity_warning`].
const POLARITY_RATIO: f32 = 1.5;

/// The share of the effective area above [`POLARITY_RATIO`] × the base, on any
/// channel, at which [`polarity_warning`] fires.
///
/// Measured 2026-10-08, each roll against its unexposed frame's base: no picture frame
/// of 11 negative-mode rolls (`polarity_probe`, which covers rolls with an `unexposed`
/// frame in the manifest) has any above 1.5×, nor, by hand against its frame 1256, the
/// Portra roll scanned in both modes; the worst reaches 1.45% above 1.25×. A slide
/// measured against its own unexposed (black) film is mostly above.
const POLARITY_MAX_SHARE: f32 = 0.01;

/// A warning when the frame does not look like a negative under `base`: a negative's
/// base is the most transparent film on it, so almost nothing may transmit more. Fires
/// for a positive (slide) scan, a base that is not this film's, or backlight past the
/// film's edge inside the area. A slide read against its clear leader passes.
/// Blind on a channel whose base is ≥ 1/1.5 of full scale: no sample can exceed it.
pub fn polarity_warning(
    image: &LinearImage,
    region: [u32; 4],
    base: &FilmBase,
) -> Result<Option<String>> {
    let share = share_above_base(image, region, base, POLARITY_RATIO)?
        .into_iter()
        .fold(0.0f32, f32::max);
    Ok((share > POLARITY_MAX_SHARE).then(|| {
        format!(
            "this does not look like a negative under the film base: {:.2}% of the \
             effective area transmits more than {POLARITY_RATIO}x the base on at least \
             one channel, where a negative's base is its most transparent film. Either \
             the base is not this film's (measure it from this roll's unexposed frame), \
             the area shows backlight past the film's edge (raise --measure-inset), or \
             the scan is a positive, which Hanten does not support",
            share * 100.0
        )
    }))
}

/// Per channel, the share of `region`'s samples that transmit more than `ratio` × the
/// base. Counts, not floating-point sums, so the result is order-independent.
fn share_above_base(
    image: &LinearImage,
    region: [u32; 4],
    base: &FilmBase,
    ratio: f32,
) -> Result<[f32; 3]> {
    check_rect(image, region)?;
    let [x, y, w, h] = region;
    let limit = <[f32; 3]>::from(*base).map(|b| b * ratio);
    let mut above = [0u64; 3];
    for row in y..y + h {
        let start = (row as usize * image.width as usize + x as usize) * 3;
        for px in image.rgb[start..start + w as usize * 3].as_chunks::<3>().0 {
            for ((a, v), l) in above.iter_mut().zip(px).zip(limit) {
                *a += u64::from(*v > l);
            }
        }
    }
    let n = (w as u64 * h as u64) as f32;
    Ok(above.map(|a| a as f32 / n))
}

/// The remedy [`guard_base`] names for a degenerate base from a stated source.
fn source_advice(source: &FilmBaseSource) -> &'static str {
    match source {
        FilmBaseSource::Region(_) => {
            "the sampled region has no usable signal on some channel (e.g. it sits on \
             the dark holder) — sample a brighter patch of unexposed film or pass \
             --film-base"
        }
        // Explicit is CLI-validated before it ever reaches here.
        FilmBaseSource::Explicit(_) => "pass a --film-base transmission in (0, 1]",
    }
}

/// Error loudly if any channel of a resolved base is non-finite or `<= 0` — such a
/// base cannot anchor the density divide. `advice` names how to recover.
fn guard_base(base: &FilmBase, advice: &str) -> Result<()> {
    let rgb = <[f32; 3]>::from(*base);
    if rgb.iter().all(|v| v.is_finite() && *v > 0.0) {
        return Ok(());
    }
    Err(NcError::Other(format!(
        "resolved film base {rgb:?} is not finite and positive on every channel; \
         it cannot anchor the density divide — {advice}"
    )))
}

/// Per-channel [`SAMPLE_PERCENTILE`] transmission over the rectangle `[x, y, w, h]`,
/// plus a uniformity warning when the rectangle is not flat (per-channel spread
/// above [`MAX_RELATIVE_SPREAD`] on any channel). A warning, not an error — a human
/// may legitimately sample an odd patch — so `--strict` can refuse it; the value is
/// unchanged by the check.
fn sample_region(image: &LinearImage, rect: [u32; 4]) -> Result<BaseEstimate> {
    let mut chans = region_channels(image, rect)?;
    let (hi, spread) = channel_stats(&mut chans);
    let mut warnings = Vec::new();
    if spread > MAX_RELATIVE_SPREAD {
        let [x, y, w, h] = rect;
        warnings.push(format!(
            "base-region [{x},{y},{w},{h}] is not uniform (worst per-channel relative \
             spread {spread:.2} > {MAX_RELATIVE_SPREAD:.2}); the rectangle may mix \
             unexposed film with image content — draw it wholly on unexposed film"
        ));
    }
    Ok(BaseEstimate {
        base: FilmBase::from(hi),
        warnings,
        percentile: Some(SAMPLE_PERCENTILE),
        spread: None,
    })
}

/// Per-channel counts of the 16-bit codes over a rectangle, and the number of
/// finite samples counted per channel.
struct CodeHistogram {
    counts: Vec<[u64; 3]>,
    n: [u64; 3],
}

impl CodeHistogram {
    /// Count the rectangle `[x, y, w, h]`, which must lie within the image (the same
    /// bounds rule as a stated region). A non-finite sample is skipped; any other is
    /// rounded to its code, which is exact for decoded data.
    fn of(image: &LinearImage, rect: [u32; 4]) -> Result<Self> {
        check_rect(image, rect)?;
        let [x, y, w, h] = rect;
        let mut counts = vec![[0u64; 3]; CODES];
        let mut n = [0u64; 3];
        let max = (CODES - 1) as f32;
        for row in y..y + h {
            let start = (row as usize * image.width as usize + x as usize) * 3;
            for px in image.rgb[start..start + w as usize * 3].as_chunks::<3>().0 {
                for (c, &v) in px.iter().enumerate() {
                    if v.is_finite() {
                        counts[(v * max).round().clamp(0.0, max) as usize][c] += 1;
                        n[c] += 1;
                    }
                }
            }
        }
        Ok(Self { counts, n })
    }

    /// The `p`-quantile of channel `c`, by the same rounded rank `round((n-1)·p)` as
    /// [`percentile`], so the two agree on decoded data. `0.0` when the channel
    /// counted nothing.
    fn percentile(&self, c: usize, p: f32) -> f32 {
        if self.n[c] == 0 {
            return 0.0;
        }
        let k = ((self.n[c] - 1) as f64 * p.clamp(0.0, 1.0) as f64).round() as u64;
        let mut seen = 0u64;
        for (code, counts) in self.counts.iter().enumerate() {
            seen += counts[c];
            if seen > k {
                return code as f32 / (CODES - 1) as f32;
            }
        }
        unreachable!("rank {k} is below the channel's count")
    }
}

// ---------------------------------------------------------------------------
// The holder march — the effective area's first cut.
// ---------------------------------------------------------------------------
//
// Where the film is IR-transparent — chromogenic dye at any exposure, and silver
// film up to the density at which accumulated silver starts blocking IR — all film
// reads bright in IR while the opaque scanner holder reads dark: a
// content-independent holder signal RGB cannot produce (holder and dense film are
// both dark in RGB).

/// The measured IR usability verdict for one frame: whether the IR plane can
/// separate the opaque holder from film *here*.
///
/// This replaces the `--film-type chromogenic` declaration that used to gate the
/// holder measurement (`ir-usability-detection`). The declaration keyed on the
/// film's chemistry; what decides separability is the frame's own accumulated
/// density, and the two disagree on exactly the frames the calibration workflow
/// uses — an *unexposed* silver frame (where `Dmin` is measured) separates ~20:1
/// while a fully-exposed silver leader (where `Dmax` is measured) is uniformly
/// opaque and cannot be told from the holder at all.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct IrSeparability {
    /// Median IR transmission over the frame interior, the border trimmed off by
    /// [`IR_USABLE_INTERIOR_MARGIN`] — "how transparent is this frame's film".
    pub interior_median: f32,
    /// Whether that median clears [`IR_USABLE_MIN_INTERIOR`], i.e. whether the
    /// film reads distinguishably brighter than the level meaning "holder".
    pub usable: bool,
}

/// Measure whether IR can separate holder from film on this frame, or `None`
/// when the scan carries no IR plane at all.
///
/// Reads only the **interior**: the holder occludes the edges, so a verdict about
/// the film must not sample the border it exists to detect. The interior is
/// strided to roughly [`IR_USABLE_MAX_SAMPLES`] values rather than materialized,
/// which keeps the check a fixed small cost at any frame size — deliberately, so
/// the hand-maintained peak-memory model in `pipeline::memory` owes it no term.
///
/// Fails safe by construction: an all-dark plane yields a low median and reports
/// unusable, so the holder goes unmeasured rather than the frame's own film being
/// classified as holder.
pub fn ir_separability(image: &LinearImage) -> Option<IrSeparability> {
    let ir = image.ir.as_deref()?;
    let (w, h) = (image.width as usize, image.height as usize);
    let margin = (w.min(h) as f32 * IR_USABLE_INTERIOR_MARGIN).round() as usize;
    // A frame too small to trim reads whole rather than empty: the verdict still
    // has to be *some* measurement of the film, and the alternative is a zero
    // sample that would always report unusable.
    let (x0, y0, x1, y1) = if 2 * margin < w.min(h) {
        (margin, margin, w - margin, h - margin)
    } else {
        (0, 0, w, h)
    };

    // Stride both axes so the sample stays bounded and evenly spread. `step` is the
    // ceiling of the square root of the reduction factor. That puts the count near
    // the cap without pinning it there: each axis rounds *up* independently, so an
    // elongated interior can sit somewhat over it (a square one lands just under).
    // The cap bounds the work, it is not an exact sample size — so allocate the
    // count actually taken rather than the cap, which would otherwise reserve
    // ~400 KB to sample a 6x6 frame.
    let interior = (x1 - x0) * (y1 - y0);
    let step = ((interior as f64 / IR_USABLE_MAX_SAMPLES as f64)
        .sqrt()
        .ceil() as usize)
        .max(1);
    let sampled = (x1 - x0).div_ceil(step) * (y1 - y0).div_ceil(step);
    let mut vals = Vec::with_capacity(sampled);
    for y in (y0..y1).step_by(step) {
        let row = y * w;
        for x in (x0..x1).step_by(step) {
            vals.push(ir[row + x]);
        }
    }

    let interior_median = percentile(&mut vals, 0.5);
    Some(IrSeparability {
        interior_median,
        usable: interior_median >= IR_USABLE_MIN_INTERIOR,
    })
}

/// The shallow near-edge depth the holder classifier probes:
/// [`IR_HOLDER_PROBE_FRAC`] of the short dimension, floored at
/// [`IR_HOLDER_PROBE_MIN`]. Shallow on purpose — see [`IR_HOLDER_PROBE_FRAC`].
fn holder_probe_depth(image: &LinearImage) -> u32 {
    ((image.width.min(image.height) as f32 * IR_HOLDER_PROBE_FRAC).round() as u32)
        .max(IR_HOLDER_PROBE_MIN)
}

/// The along-edge length of `edge` (columns for top/bottom, rows for left/right).
fn along_len(image: &LinearImage, edge: Edge) -> u32 {
    match edge {
        Edge::Top | Edge::Bottom => image.width,
        Edge::Left | Edge::Right => image.height,
    }
}

/// Median IR transmission over one band of the holder march: the
/// `thickness`-deep strip starting `depth` px in from `edge`, spanning along-edge
/// `[along_lo, along_hi)`. The median resists dust and hot IR pixels.
///
/// `buf` is caller-owned so the march reuses one allocation across its bands
/// instead of allocating per step. It is cleared on entry; its contents on return
/// are scratch.
#[allow(clippy::too_many_arguments)]
fn median_ir_band(
    image: &LinearImage,
    ir: &[f32],
    edge: Edge,
    along_lo: u32,
    along_hi: u32,
    depth: u32,
    thickness: u32,
    buf: &mut Vec<f32>,
) -> f32 {
    let (w, h) = (image.width, image.height);
    let [x, y, rw, rh] = match edge {
        Edge::Top => [along_lo, depth, along_hi - along_lo, thickness],
        Edge::Bottom => [
            along_lo,
            h - depth - thickness,
            along_hi - along_lo,
            thickness,
        ],
        Edge::Left => [depth, along_lo, thickness, along_hi - along_lo],
        Edge::Right => [
            w - depth - thickness,
            along_lo,
            thickness,
            along_hi - along_lo,
        ],
    };
    buf.clear();
    buf.reserve((rw as usize) * (rh as usize));
    for row in y..y + rh {
        let row_start = row as usize * w as usize;
        for col in x..x + rw {
            buf.push(ir[row_start + col as usize]);
        }
    }
    percentile(buf, 0.5)
}

/// The **effective measurement area** of one frame: the rectangle a measurement
/// may be computed over, after the opaque holder and a static border inset have
/// been removed (`film-base/holder-depth-mask`).
///
/// It is a **per-edge rectangle, not a mask**. A holder covering only part of one
/// edge widens that whole edge to its deepest segment — the accepted cost of
/// letting every consumer clamp its walk to bounds instead of testing each pixel,
/// which is what keeps `pipeline::memory` free of a mask buffer.
///
/// The image is **never cropped**: this describes where a statistic is read, not
/// what is written.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct EffectiveArea {
    /// The resolved rectangle `[x, y, w, h]`, in the same convention as
    /// `--base-region`.
    pub region: [u32; 4],
    /// The measured holder depths, or `None` when the holder was **not measured**
    /// (no IR plane, a shape-only plane, or IR that does not separate on this
    /// frame). `Some` with all-zero depths is a different answer: the holder *was*
    /// measured and there is none, which is what an already-cropped scan looks
    /// like. The report must keep the two apart — "no holder found" is a verdict,
    /// "not measured" is not.
    ///
    /// **Premise behind the all-zero verdict, recorded because it is load-bearing
    /// and unstated elsewhere:** the gate that lets the march run at all is
    /// [`ir_separability`], which samples only the frame *interior* (border trimmed
    /// by [`IR_USABLE_INTERIOR_MARGIN`]) precisely so a verdict about the film does
    /// not sample the border it exists to detect. So `usable: true` asserts nothing
    /// about whether an IR-opaque holder exists, and "measured, and there is no
    /// holder" therefore rests on the march's own outermost band alone, with no
    /// corroborating evidence. It is asserted, not falsified. What makes that
    /// tolerable is the physical premise that IR opacity comes from material thickness,
    /// not colour, so even a light holder blocks 850-950 nm — which is why no plausible failing case has been
    /// constructed — not a second measurement.
    pub holder: Option<HolderDepths>,
    /// Whether the holder measurement **moved this rectangle**: the holder was
    /// measured *and* at least one edge's depth is non-zero.
    ///
    /// Returned rather than left to the caller, because it is the fact callers
    /// actually want — "did consuming the IR plane change anything?" — and
    /// recomputing a stage's outcome downstream from its inputs has gone wrong
    /// before. `holder: Some` with
    /// all-zero depths reads `false`: the plane was read and the answer was "no
    /// holder", which moved nothing.
    pub holder_applied: bool,
    /// The static inset **applied** inside the holder, in pixels — the same value
    /// on every edge, taken from the **original** frame's shorter dimension so it
    /// does not move with the holder measurement.
    ///
    /// The applied value, not the requested one: on a frame where the holder was
    /// measured it is floored at one holder-probe step (see [`effective_area`]), so
    /// a stated fraction near `0` can report more than it asked for. That is the
    /// point of reporting it.
    pub inset: u32,
}

/// Which edges' marches reached [`HOLDER_MARCH_MAX_FRAC`] without finding film.
///
/// Per edge, so the report and the warning say where the IR read went wrong. An
/// edge can cap because a perpendicular edge capped (see [`HOLDER_MARCH_MAX_FRAC`]);
/// either way its depth is the cap, not a measurement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct CappedEdges {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl CappedEdges {
    /// The four flags in the depth array's order (top, bottom, left, right), so
    /// callers indexing the depths can index these the same way.
    fn from_array([top, bottom, left, right]: [bool; 4]) -> Self {
        Self {
            top,
            bottom,
            left,
            right,
        }
    }

    /// Whether any edge capped — the question the old frame-wide boolean answered.
    pub fn any(self) -> bool {
        self.top || self.bottom || self.left || self.right
    }

    /// The capped edges' names, in report order, for a warning that names them.
    pub fn names(self) -> Vec<&'static str> {
        [
            (self.top, "top"),
            (self.bottom, "bottom"),
            (self.left, "left"),
            (self.right, "right"),
        ]
        .into_iter()
        .filter_map(|(hit, name)| hit.then_some(name))
        .collect()
    }
}

/// Per-edge holder depth in pixels, measured inward from each edge.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct HolderDepths {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
    /// Which edges' marches reached [`HOLDER_MARCH_MAX_FRAC`] without finding film.
    /// A capped edge reports the cap, not a measurement: no holder is that deep, so
    /// the region is over-cut rather than holding holder (see
    /// [`HOLDER_MARCH_MAX_FRAC`]).
    pub capped: CappedEdges,
    /// Whether the fixed-point march *settled* — a pass reproduced the previous
    /// pass's depths — rather than exhausting [`HOLDER_MARCH_PASSES`].
    ///
    /// **Read this together with `capped`, not as an independent quality signal.**
    /// A cap *creates* a stable fixed point — identical trims give an identical
    /// measurement — so a capped frame, its perpendicular edges inflated to the cap
    /// with it, comes back `converged: true`.
    /// `converged` says the iteration settled; it says nothing about whether what it
    /// settled on is a measurement.
    ///
    /// `false` is not an error, but it is not a settled measurement either: the
    /// edge-depth map is not monotone (a deeper trim narrows the along-edge
    /// segments, which can make a partially-covered segment read holder where a
    /// wider one read film), so nothing rules out a cycle. On exhaustion
    /// [`holder_depths`] reports the elementwise **max** of the last two passes.
    ///
    /// That guarantees an over-cut (costing measurement area) rather than an
    /// under-cut (leaving holder inside the region) for a **two-phase** cycle, whose
    /// last two passes are every phase there is, and for a transient still settling
    /// **downward**, where the earlier of the two passes is the deeper one. It does
    /// *not* cover a transient settling upward: `max(p3, p4) = p4` then sits below
    /// the fixed point, which is an under-cut. (Unlikely in practice for the same
    /// reason the merge is not widened below: pass 1 is measured untrimmed and is
    /// the corner-contaminated upper bound, so the sequence starts high.) A
    /// period-≥3 cycle can likewise still under-cut:
    /// passes 3 and 4 are only two of its phases, and a third may carry the deeper
    /// value on some edge. Widening the merge to all four passes is *not* the fix —
    /// pass 1 is the untrimmed, corner-contaminated upper bound, so it would
    /// over-cut every ordinary ring frame.
    ///
    /// The over-cut is not free either: the merge can raise `top + bottom` to the
    /// frame height (or `left + right` to its width), which with the inset added
    /// trips [`effective_area`]'s empty-region error. The orchestrator decides how
    /// loud that is — exit 2 on a `measure-base` that measures over the area, a warning
    /// with no reported area otherwise. So a far enough over-cut is a hard refusal
    /// for a run that would have read it, rather than a degraded measurement.
    pub converged: bool,
}

/// Resolve the [`EffectiveArea`]: measure the holder where IR permits, then inset
/// what remains.
///
/// **Two cuts, in order — never one or the other.** The inset is not a fallback for
/// a missing holder measurement: it runs on every path, and where the holder could
/// not be measured it is simply the only cut. Sizing it is the user's call
/// (`inset_frac`, default [`crate::types::DEFAULT_MEASURE_INSET`]) precisely because nc declines to
/// guess a holder depth it cannot measure.
///
/// `inset_frac` is a fraction of the **original** frame's shorter dimension, so a
/// given scan insets the same pixel count whatever the holder measured. Where the
/// holder *was* measured the resolved inset is floored at one holder-probe step,
/// which is the holder cut's own resolution — see the comment on the arithmetic;
/// [`EffectiveArea::inset`] reports the applied value.
///
/// Takes the resolved fraction rather than reading a config, and returns a region
/// per **call** — not one region per frame. Every caller today happens to resolve
/// it once per frame, which is fine; the constraint is only that the signature not
/// *preclude* a per-usage region (`holder-depth-mask`), since a measurement handed
/// a user-stated region does not call this at all and a future per-usage
/// distinction must be able to extend the arguments instead of rewriting every
/// call site.
pub fn effective_area(image: &LinearImage, inset_frac: f32) -> Result<EffectiveArea> {
    // The same check `cli::validate_shared` runs, so a programmatic caller cannot bypass
    // the bound and the two gates cannot disagree about it.
    check_measure_inset(inset_frac)?;
    let holder = holder_depths(image);
    let requested = (image.width.min(image.height) as f32 * inset_frac).round() as u32;
    // **The inset carries the holder cut's own resolution.** A march reports the
    // *start* of the first band whose median read film, and a film median only
    // means the holder covers less than half that band — so up to ~`step/2` of
    // holder can sit inboard of any measured depth, including a measured **zero**
    // ("no holder in the outermost band" is not "no holder"). So where the march
    // ran, the inset is floored at one probe step and the mixed band is absorbed by
    // construction. Same spirit as [`IR_HOLDER_PROBE_MIN`] flooring the probe
    // itself.
    //
    // At the default it never binds (a 3600 px frame insets 180 px against an 18 px
    // step); it takes effect only near `--measure-inset 0`, where the residual is
    // otherwise the whole defence — a 9-18 px holder ring is ~1-2% of the region at
    // `SCAN_EPSILON` density, the size of the top 1% `hanten measure-roll`'s pooled
    // white (`roll_white::PERCENTILE`, p99) reads. (It was sized against the per-frame
    // auto `Dmax`'s p99.5, which retired.) A
    // stated `0` is therefore not honoured exactly on a measured frame; that is
    // deliberate, and [`EffectiveArea::inset`] reports the **applied** value so the
    // floor is visible rather than silent. Where the holder was *not* measured
    // there is no measurement resolution to respect, and the stated fraction is
    // used as-is.
    let inset = match holder {
        Some(_) => requested.max(holder_probe_depth(image)),
        None => requested,
    };
    let (top, bottom, left, right) = match holder {
        Some(h) => (
            h.top + inset,
            h.bottom + inset,
            h.left + inset,
            h.right + inset,
        ),
        None => (inset, inset, inset, inset),
    };
    // Recorded here, where the arithmetic happens, so no caller has to re-derive
    // "did the IR plane change anything?".
    let holder_applied =
        holder.is_some_and(|h| [h.top, h.bottom, h.left, h.right].iter().any(|&d| d != 0));
    if left + right >= image.width || top + bottom >= image.height {
        // The measured depths and the inset are printed as **separate** quantities.
        // `top`/`bottom`/`left`/`right` above are the post-inset totals, and
        // labelling those "holder depths" sent a reader after a 110 px holder that
        // measured 30.
        let measured = match holder {
            Some(h) => format!(
                "measured holder depths (top {}, bottom {}, left {}, right {})",
                h.top, h.bottom, h.left, h.right
            ),
            None => "no holder measurement".to_string(),
        };
        // Naming "lower the fraction" when the probe-step floor is what set the
        // inset would be a remedy that cannot work.
        let remedy = if inset > requested {
            format!(
                "The inset is the {inset} px holder-probe step, its floor on a frame \
                 where the holder was measured, so lowering the fraction (currently \
                 {inset_frac}) cannot shrink it — the measured holder alone leaves \
                 too little to measure over."
            )
        } else {
            format!("Lower the inset fraction (currently {inset_frac}).")
        };
        return Err(NcError::Usage(format!(
            "the measurement region is empty: on a {}x{} frame, {measured} plus a \
             {inset} px inset on every edge leave nothing to measure. {remedy}",
            image.width, image.height
        )));
    }
    Ok(EffectiveArea {
        region: [
            left,
            top,
            image.width - left - right,
            image.height - top - bottom,
        ],
        holder,
        holder_applied,
        inset,
    })
}

/// The warnings a resolved [`EffectiveArea`] carries out of the stage, in report
/// order.
///
/// Both of them mean "the reported rectangle is not a measurement", which a
/// `Serialize`-only field would leave unseen. Every command that resolves the area
/// pushes these, so `--strict` promotes them like any other report warning.
///
/// A pure function rather than a field on [`EffectiveArea`], because that struct is
/// serialized straight into the report and warnings ride the report's own
/// `warnings` array.
pub fn effective_area_warnings(area: &EffectiveArea) -> Vec<String> {
    let mut out = Vec::new();
    let Some(h) = area.holder else {
        return out;
    };
    if h.capped.any() {
        let edges = h.capped.names().join(", ");
        let depth_pct = (HOLDER_MARCH_MAX_FRAC * 100.0).round();
        // The premise and its remedy are `HOLDER_MARCH_MAX_FRAC`'s.
        out.push(format!(
            "the film-holder depth march hit its cap ({depth_pct}% of the shorter \
             edge) on the {edges} edge(s) without finding film. No film holder is \
             that deep, so the IR read there is not holder (IR-dark film or debris) \
             and `effective_area.holder` reports the cap, not a measurement; an edge \
             perpendicular to a capped one can be inflated with it. The error only \
             over-cuts: the region loses area but holds no holder. If this frame's \
             holder really is that deep, raise the inset (--measure-inset, or \
             `measure.inset`), which is added on top of the cap."
        ));
    }
    if !h.converged {
        out.push(format!(
            "the film-holder depth march did not settle within \
             {HOLDER_MARCH_PASSES} passes, so `effective_area.holder` reports the \
             deeper of the last two passes, which is not a settled measurement. \
             That over-cuts — costing measurement area — for a two-phase cycle or \
             a march still settling downward, but a longer cycle or one settling \
             upward can sit below the fixed point and leave holder inside the \
             region. Every statistic read over `effective_area.region` inherits \
             that."
        ));
    }
    out
}

/// March inward from each edge and return where the opaque holder ends, or `None`
/// when this frame's IR cannot answer.
///
/// `None` means **not measured**: no IR plane, a plane identified by shape alone
/// (unverified provenance must not be thresholded), or [`ir_separability`]
/// measuring that this frame's own film is too IR-opaque to be told from the
/// holder. Consume that verdict; do not re-derive it from "is there an IR plane".
/// Silver film declines routinely — it blocks IR in proportion to accumulated
/// density — so the not-measured path is normal for B&W, not an edge case.
///
/// A holder that reads holder along *every* segment of every edge is the normal
/// case (22 of 25 real chromogenic frames): it wraps the border, and the answer is
/// how deep it goes.
///
/// Per edge, each along-edge segment ([`IR_HOLDER_SEGMENTS`]) marches in [`holder_probe_depth`] steps until a band reads film; the edge's
/// depth is the **deepest** segment, since the result is a rectangle. A segment
/// with no holder stops at the first band, so an uncropped-but-clear frame costs
/// one shallow pass.
pub fn holder_depths(image: &LinearImage) -> Option<HolderDepths> {
    let ir = image.ir.as_deref()?;
    // Trust the plane only when its provenance is marker-verified: a stray grayscale
    // page thresholded as IR could silently move every measurement that starts from
    // this region.
    if !image.ir_verified {
        return None;
    }
    if !ir_separability(image).is_some_and(|s| s.usable) {
        return None;
    }
    let step = holder_probe_depth(image);
    // A frame too small to fit one probe band within half an edge cannot be
    // measured at all: [`march_edge_depth`]'s `while depth + step <= limit` would
    // never run, so every segment would take the not-cleared arm and the edge would
    // report the cap with `capped: true` on *no* band read. "Not measured" is the
    // honest verdict, and it is a state every caller already handles.
    if image.width.min(image.height) / 2 < step {
        return None;
    }
    // Each edge is measured only over the along-edge positions that survive the
    // **perpendicular** edges' cuts. Without that, a holder *ring* — the normal
    // case — makes the corner columns read holder for the frame's entire height, so
    // every edge marches to its cap and the rectangle collapses.
    //
    // The trims are the other edges' depths, which are what we are computing, so it
    // is a fixed point: start untrimmed (the corner-contaminated upper bound) and
    // feed each pass's depths in as the next pass's trims. On every frame measured
    // so far it settles in two or three passes, pass 1 over-reporting and pass 2
    // reading the true depths — but that is an observation, **not** a proof. The
    // map is not monotone: the segment partition is recomputed from the *trimmed*
    // extent, so a deeper trim makes segments narrower, which can make a
    // partially-covered segment read holder where a wider one read film. A depth
    // can therefore rise as the trim rises, and nothing excludes a cycle.
    //
    // So the loop reports whether it settled (`converged`), and on exhaustion
    // returns the elementwise **max** of the last two passes, which over-cuts
    // (costing measurement area) for a two-phase cycle or a march still settling
    // downward. It is not a guarantee in either direction — see
    // [`HolderDepths::converged`] for the cases that can still under-cut.
    let (depths, capped, converged) = march_to_fixed_point(image, ir, step, HOLDER_MARCH_PASSES);
    Some(HolderDepths {
        top: depths[0],
        bottom: depths[1],
        left: depths[2],
        right: depths[3],
        capped: CappedEdges::from_array(capped),
        converged,
    })
}

/// The fixed-point march itself: returns `(depths, capped, converged)`, `capped`
/// per edge in the depths array's order.
///
/// Split out of [`holder_depths`] with `passes` as a parameter purely so the
/// exhaustion path is *reachable* — no fixture exhibits a cycle, so at
/// [`HOLDER_MARCH_PASSES`] the `!converged` merge would never run under test.
/// `passes = 1` reaches it on any frame with a holder: pass 1 cannot repeat the
/// all-zero start, so the merge runs against it.
fn march_to_fixed_point(
    image: &LinearImage,
    ir: &[f32],
    step: u32,
    passes: usize,
) -> ([u32; 4], [bool; 4], bool) {
    let mut depths = [0u32; 4];
    let mut capped = [false; 4];
    let mut converged = false;
    let mut last: Option<([u32; 4], [bool; 4])> = None;
    for _ in 0..passes {
        let mut next = [0u32; 4];
        let mut next_capped = [false; 4];
        for (slot, edge) in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right]
            .into_iter()
            .enumerate()
        {
            // Top/bottom run along x, so they are trimmed by left/right; left/right
            // run along y and are trimmed by top/bottom.
            let (trim_lo, trim_hi) = match edge {
                Edge::Top | Edge::Bottom => (depths[2], depths[3]),
                Edge::Left | Edge::Right => (depths[0], depths[1]),
            };
            let (depth, hit_cap) = march_edge_depth(image, ir, edge, step, trim_lo, trim_hi);
            next[slot] = depth;
            next_capped[slot] = hit_cap;
        }
        let prev = (depths, capped);
        depths = next;
        capped = next_capped;
        if next == prev.0 {
            converged = true;
            break;
        }
        // Not settled *yet*. Keep the previous pass so that, if this turns out to
        // be the last one, the answer can be the elementwise max of the two rather
        // than whichever phase the final pass happened to land on.
        last = Some(prev);
    }
    if !converged && let Some((prev, prev_capped)) = last {
        depths = merge_unsettled_passes(depths, prev);
        // Elementwise, matching the depth merge: an edge whose reported depth came
        // from the earlier pass must carry that pass's cap flag with it.
        for (slot, hit) in prev_capped.into_iter().enumerate() {
            capped[slot] |= hit;
        }
    }
    (depths, capped, converged)
}

/// What an unsettled march reports: the elementwise **max** of the last two
/// passes.
///
/// A cycle between two phases has no right answer, so the choice is which way to
/// be wrong. Over-cutting costs measurement area; under-cutting leaves holder
/// inside the region and corrupts every statistic read from it. Its own function
/// so the rule is testable — no fixture exhibits a cycle.
fn merge_unsettled_passes(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

/// The holder depth on one edge: the deepest of its along-edge segments, and
/// whether any segment hit the march cap without finding film.
///
/// `trim_lo` / `trim_hi` drop that many pixels from each end of the along-edge
/// extent — the perpendicular edges' depths, so a corner that belongs to another
/// edge's cut does not inflate this one (see [`holder_depths`]). A trim wider than
/// the edge is ignored rather than yielding an empty extent: a degenerate frame
/// still gets *a* measurement, and the caller's own region guard is what refuses.
fn march_edge_depth(
    image: &LinearImage,
    ir: &[f32],
    edge: Edge,
    step: u32,
    trim_lo: u32,
    trim_hi: u32,
) -> (u32, bool) {
    let along_full = along_len(image, edge);
    let (along_lo, along_hi) = if trim_lo + trim_hi < along_full {
        (trim_lo, along_full - trim_hi)
    } else {
        (0, along_full)
    };
    // [`IR_HOLDER_SEGMENTS`] segments spread **evenly** over the extent, every one
    // within a pixel of the others.
    //
    // Not a floor-plus-leftover split: its narrow remainder segment, reduced here by
    // a `max`, drags the whole edge to the cap. Measured on Portra160 `1102`: a **6 px** trailing segment never
    // cleared, reporting the left holder as the 900 px cap where the other 24
    // segments agreed on 108-126 — a 7x over-cut that discarded 15% of the frame
    // width, with `capped` the only hint anything was wrong.
    let extent = (along_hi - along_lo) as u64;
    let bound = |i: u64| along_lo + (extent * i / IR_HOLDER_SEGMENTS as u64) as u32;
    let perpendicular = match edge {
        Edge::Top | Edge::Bottom => image.height,
        Edge::Left | Edge::Right => image.width,
    };
    // Never march past half the frame from one edge, whatever the fraction says —
    // two opposing edges at the cap must still leave a region. `holder_depths`
    // declines a frame too small for `step` to fit inside that clamp, so the loop
    // below always reads at least one band and `capped` is never set on no
    // evidence.
    let limit = ((image.width.min(image.height) as f32 * HOLDER_MARCH_MAX_FRAC).round() as u32)
        .max(step)
        .min(perpendicular / 2);

    let mut buf = Vec::new();
    let mut deepest = 0u32;
    let mut capped = false;
    for i in 0..IR_HOLDER_SEGMENTS as u64 {
        let (start, end) = (bound(i), bound(i + 1));
        if start >= end {
            // Fewer pixels than segments: the extra segments are empty.
            continue;
        }
        let mut depth = 0u32;
        let mut cleared = false;
        while depth + step <= limit {
            let med = median_ir_band(image, ir, edge, start, end, depth, step, &mut buf);
            if med > IR_HOLDER_MAX_TRANSMISSION {
                cleared = true;
                break;
            }
            depth += step;
        }
        if !cleared {
            // This segment is holder as deep as we looked. Report the cap as a
            // floor and say so, rather than pretending the holder ends there.
            capped = true;
            depth = limit;
        }
        deepest = deepest.max(depth);
    }
    (deepest, capped)
}

/// Per-channel high percentile and the worst per-channel relative spread
/// `(p_hi - p_lo) / p_hi` over gathered channel samples. A zero/negative high
/// percentile yields spread 1.0 (maximally non-uniform) so degenerate data can
/// never look confident.
fn channel_stats(chans: &mut [Vec<f32>; 3]) -> ([f32; 3], f32) {
    let mut hi = [0.0f32; 3];
    let mut spread = 0.0f32;
    for (c, samples) in chans.iter_mut().enumerate() {
        let h = percentile(samples, SAMPLE_PERCENTILE);
        let l = percentile(samples, LOW_PERCENTILE);
        hi[c] = h;
        spread = spread.max(if h > 0.0 { (h - l) / h } else { 1.0 });
    }
    (hi, spread)
}

/// Refuse a rectangle `[x, y, w, h]` that is empty or leaves the image — a usage
/// error rather than a clamp, so a bad `--base-region` fails loudly.
fn check_rect(image: &LinearImage, [x, y, w, h]: [u32; 4]) -> Result<()> {
    if w == 0 || h == 0 {
        return Err(NcError::Usage(format!(
            "base-region must be non-empty (got {w}x{h})"
        )));
    }
    // Use u64 for the right edge so a region near u32::MAX can't wrap.
    let (right, bottom) = (x as u64 + w as u64, y as u64 + h as u64);
    if right > image.width as u64 || bottom > image.height as u64 {
        return Err(NcError::Usage(format!(
            "base-region [{x},{y},{w},{h}] is outside the {}x{} image",
            image.width, image.height
        )));
    }
    Ok(())
}

/// Gather the rectangle `[x, y, w, h]` into per-channel sample vectors, after
/// [`check_rect`].
fn region_channels(image: &LinearImage, rect: [u32; 4]) -> Result<[Vec<f32>; 3]> {
    check_rect(image, rect)?;
    let [x, y, w, h] = rect;
    let mut chans: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let cap = (w as usize) * (h as usize);
    for c in &mut chans {
        c.reserve(cap);
    }
    for row in y..y + h {
        let row_start = (row as usize * image.width as usize + x as usize) * 3;
        for col in 0..w as usize {
            let i = row_start + col * 3;
            chans[0].push(image.rgb[i]);
            chans[1].push(image.rgb[i + 1]);
            chans[2].push(image.rgb[i + 2]);
        }
    }
    Ok(chans)
}

/// The `p`-quantile (0.0–1.0) of `values` by rounded rank `round((n-1)·p)` over
/// the finite values, no interpolation, in O(n). (Not the textbook nearest-rank
/// `⌈p·n⌉`: for `[0.1,0.2,0.3,0.4]` at p=0.5 this returns `0.3`, not `0.2`.)
///
/// Non-finite samples (`NaN`, `±inf`) are dropped first, so a stray non-finite
/// pixel can never be returned as the base (which would poison the density
/// divide downstream); the rank is then an order statistic
/// (`select_nth_unstable_by` under the `f32::total_cmp` total order), whose
/// value is independent of tie order — deterministic by construction. Empty /
/// all-non-finite input yields `0.0`. In practice decoded samples are always
/// finite `[0, 1]`; this just makes the helper sound if reused.
fn percentile(values: &mut Vec<f32>, p: f32) -> f32 {
    values.retain(|v| v.is_finite());
    if values.is_empty() {
        return 0.0;
    }
    // f64 for the index: a region can exceed 2^24 samples (a 24 MP interior),
    // above which an `as f32` rank cast loses integer precision and would pick a
    // slightly wrong order statistic. f64 is exact here with no measurable cost.
    let k = ((values.len() - 1) as f64 * p.clamp(0.0, 1.0) as f64).round() as usize;
    *values.select_nth_unstable_by(k, |a, b| a.total_cmp(b)).1
}

/// The **frozen** synthetic scan the `pipeline_version` drift gate fingerprints
/// stage 2 over (`crate::version`), and the region it reads.
///
/// A conversion's base is either stated outright or read from a stated region at
/// [`SAMPLE_PERCENTILE`]; the region path is the one that reads pixels, and the
/// render fingerprint (handed a hardcoded base) cannot see it. So the gate hashes
/// [`estimate`] over [`golden::REGION`] of this scan. The effective-area measurement
/// is not in a conversion — its result reaches one as an explicit base — so it is
/// not fingerprinted.
///
/// **Do not edit [`golden::scan`] or [`golden::REGION`].** They are frozen inputs:
/// changing them moves the fingerprint exactly as changing the estimator would.
///
/// **Why hashing this is safe on both macOS/aarch64 and x86_64 Linux** (design-spec
/// §8): the region path is integer indexing, comparisons and rounded-rank order
/// statistics ([`percentile`], whose result is the k-th smallest *value* and so is
/// independent of tie order). No libm transcendental runs on it.
#[cfg(test)]
pub(crate) mod golden {
    use super::*;

    /// Measured rebate transmission of the user's real film stock, and a
    /// representative opaque-holder transmission.
    const REBATE: [f32; 3] = [0.53, 0.26, 0.16];
    const HOLDER: [f32; 3] = [0.01, 0.01, 0.01];

    /// The region the drift gate reads: the bottom rebate band, inside the side
    /// holder.
    pub(crate) const REGION: [u32; 4] = [3, 93, 94, 4];

    /// The frozen 100×100 layout: dark holder ring (3 px) → thin unexposed rebate
    /// band (4 px, bottom and left) → varied gradient picture interior, the real
    /// `holder → thin rebate → picture` geometry.
    pub(crate) fn scan() -> LinearImage {
        let (w, h) = (100u32, 100u32);
        let mut buf = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let t = (x + y) as f32 / (w + h) as f32;
                buf.extend_from_slice(&[0.05 + 0.35 * t, 0.03 + 0.20 * t, 0.02 + 0.10 * t]);
            }
        }
        let mut img = LinearImage::new(w, h, buf, None).unwrap();
        for rect in [
            [0, 0, w, 3],
            [0, h - 3, w, 3],
            [0, 0, 3, h],
            [w - 3, 0, 3, h],
        ] {
            fill(&mut img, rect, HOLDER);
        }
        // Bands ripple **along** the edge (bottom by x, left by y), so the region
        // is textured to the percentile.
        for x in 0..w {
            for y in h - 7..h - 3 {
                set(&mut img, x, y, rebate_at(x));
            }
        }
        for y in 0..h {
            for x in 3..7 {
                set(&mut img, x, y, rebate_at(y));
            }
        }
        img
    }

    /// The rebate transmission at along-edge position `step`.
    ///
    /// The band is deliberately **not** flat. A perfectly uniform band returns the
    /// same value for *any* percentile, so retuning [`SAMPLE_PERCENTILE`] — one of
    /// the exact changes this fingerprint exists to catch — would leave the hash
    /// unmoved. A 7% ripple over ten levels stays inside [`MAX_RELATIVE_SPREAD`]
    /// while putting the 97th percentile and, say, the 90th on different levels.
    fn rebate_at(step: u32) -> [f32; 3] {
        let f = 0.93 + 0.07 * (step % 10) as f32 / 9.0;
        [REBATE[0] * f, REBATE[1] * f, REBATE[2] * f]
    }

    fn fill(img: &mut LinearImage, [x, y, w, h]: [u32; 4], rgb: [f32; 3]) {
        for yy in y..y + h {
            for xx in x..x + w {
                set(img, xx, yy, rgb);
            }
        }
    }

    fn set(img: &mut LinearImage, x: u32, y: u32, rgb: [f32; 3]) {
        let i = ((y * img.width + x) * 3) as usize;
        img.rgb[i..i + 3].copy_from_slice(&rgb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_above_base_counts_per_channel_over_the_region_only() {
        let mut img = solid(10, 10, [0.2, 0.2, 0.2]);
        // A quarter of the region over the base on red only; outside it, everything.
        fill_rect(&mut img, [0, 0, 2, 10], [1.0, 1.0, 1.0]);
        fill_rect(&mut img, [2, 0, 2, 5], [0.5, 0.2, 0.2]);
        let base = FilmBase::from([0.3, 0.3, 0.3]);
        let share = share_above_base(&img, [2, 0, 4, 10], &base, 1.5).unwrap();
        assert_eq!(share, [0.25, 0.0, 0.0]);
        assert!(share_above_base(&img, [8, 8, 4, 4], &base, 1.5).is_err());
    }

    #[test]
    fn polarity_warning_fires_only_past_the_share() {
        let base = FilmBase::from([0.3, 0.3, 0.3]);
        let mut img = solid(100, 100, [0.2, 0.2, 0.2]);
        // 1% over 1.5x the base on one channel is still a negative's noise.
        fill_rect(&mut img, [0, 0, 1, 100], [0.2, 0.2, 0.5]);
        assert_eq!(
            polarity_warning(&img, [0, 0, 100, 100], &base).unwrap(),
            None
        );
        fill_rect(&mut img, [1, 0, 1, 100], [0.2, 0.2, 0.5]);
        let w = polarity_warning(&img, [0, 0, 100, 100], &base)
            .unwrap()
            .unwrap();
        assert!(w.contains("2.00% of the effective area"), "{w}");
    }

    /// Build an `w`x`h` image filled with a flat RGB color.
    fn solid(w: u32, h: u32, rgb: [f32; 3]) -> LinearImage {
        let mut buf = Vec::with_capacity((w * h * 3) as usize);
        for _ in 0..w * h {
            buf.extend_from_slice(&rgb);
        }
        LinearImage::new(w, h, buf, None).unwrap()
    }

    /// Set one pixel's RGB in place.
    fn set_px(img: &mut LinearImage, x: u32, y: u32, rgb: [f32; 3]) {
        let i = ((y * img.width + x) * 3) as usize;
        img.rgb[i..i + 3].copy_from_slice(&rgb);
    }

    /// Fill a rectangle with a flat RGB color.
    fn fill_rect(img: &mut LinearImage, [x, y, w, h]: [u32; 4], rgb: [f32; 3]) {
        for yy in y..y + h {
            for xx in x..x + w {
                set_px(img, xx, yy, rgb);
            }
        }
    }

    /// The measured rebate transmission of the user's real film stock
    /// (`48bit-full/1` bottom edge ≈ `48bit-full/2` left edge).
    const REBATE: [f32; 3] = [0.53, 0.26, 0.16];

    fn assert_close(base: FilmBase, want: [f32; 3], tol: f32) {
        for (got, want) in <[f32; 3]>::from(base).iter().zip(want) {
            assert!((got - want).abs() < tol, "got {base:?}, want {want:?}");
        }
    }

    #[test]
    fn explicit_source_returns_value_verbatim() {
        // A tiny dark image still resolves: the explicit value reads no pixels.
        let img = solid(4, 4, [0.1, 0.1, 0.1]);
        let est = estimate(&img, &FilmBaseSource::Explicit([0.9, 0.55, 0.42])).unwrap();
        assert_eq!(est.base, FilmBase::from([0.9, 0.55, 0.42]));
        assert!(est.warnings.is_empty());
        assert_eq!(est.percentile, None);
    }

    #[test]
    fn region_source_samples_the_rectangle() {
        // Bright interior region, dark border: sampling the region must pick the
        // region's value rather than the surrounding frame.
        let mut img = solid(10, 10, [0.2, 0.2, 0.2]);
        fill_rect(&mut img, [4, 4, 2, 2], [0.8, 0.6, 0.5]);
        let est = estimate(&img, &FilmBaseSource::Region([4, 4, 2, 2])).unwrap();
        assert_close(est.base, [0.8, 0.6, 0.5], 1e-6);
        // A flat rectangle raises no uniformity warning.
        assert!(est.warnings.is_empty(), "{:?}", est.warnings);
    }

    #[test]
    fn mixed_region_warns_but_keeps_the_value() {
        // A rectangle straddling rebate and picture yields a plausible-looking
        // p97 — the uniformity warning is the only signal, and the value must
        // not be silently altered by the check.
        let mut img = solid(20, 20, [0.2, 0.1, 0.05]);
        fill_rect(&mut img, [0, 0, 20, 6], REBATE); // top: fake rebate
        let mixed = estimate(&img, &FilmBaseSource::Region([0, 0, 20, 12])).unwrap();
        assert!(
            mixed.warnings.iter().any(|w| w.contains("not uniform")),
            "mixed rectangle must warn: {:?}",
            mixed.warnings
        );
        assert_close(mixed.base, REBATE, 1e-6); // p97 lands on the bright part, unchanged
        // The clean sub-rectangle does not warn.
        let clean = estimate(&img, &FilmBaseSource::Region([0, 0, 20, 6])).unwrap();
        assert!(clean.warnings.is_empty(), "{:?}", clean.warnings);
    }

    #[test]
    fn the_frozen_drift_gate_region_resolves_cleanly_and_is_percentile_sensitive() {
        // The stage-2 drift fingerprint (`crate::version::PipelineFingerprint`)
        // hashes `estimate` over `golden::REGION` of `golden::scan`. Two properties
        // make that hash meaningful, and neither is self-evident from the fixture:
        let img = golden::scan();

        // (1) it resolves cleanly — a fixture that errored or warned would
        //     fingerprint the failure path instead of the estimator.
        let est = estimate(&img, &FilmBaseSource::Region(golden::REGION)).unwrap();
        assert!(est.warnings.is_empty(), "{:?}", est.warnings);

        // (2) the band is textured, so the CHOSEN percentile is observable: a flat
        //     band returns the same value for every percentile, and a retuned
        //     SAMPLE_PERCENTILE would then leave the hash unmoved.
        let [mut r, ..] = region_channels(&img, golden::REGION).unwrap();
        assert_ne!(
            percentile(&mut r, 0.90),
            percentile(&mut r, SAMPLE_PERCENTILE),
            "the frozen band must not be flat, or the fingerprint cannot see a \
             retuned SAMPLE_PERCENTILE"
        );
    }

    #[test]
    fn high_percentile_resists_hot_pixels() {
        // A handful of blown-out pixels in the region must not pull the estimate
        // up to the max — the 97th percentile stays near the true base.
        let mut img = solid(10, 10, [0.5, 0.5, 0.5]);
        for x in 0..3 {
            set_px(&mut img, x, 0, [9.0, 9.0, 9.0]);
        }
        let est = sample_region(&img, [0, 0, 10, 10]).unwrap();
        assert!(est.base.r < 1.0, "hot pixels leaked in: {}", est.base.r);
        assert!((est.base.r - 0.5).abs() < 1e-6);
    }

    #[test]
    fn out_of_bounds_region_is_usage_error() {
        let img = solid(8, 8, [0.5, 0.5, 0.5]);
        let err = estimate(&img, &FilmBaseSource::Region([4, 4, 8, 8])).unwrap_err();
        assert!(matches!(err, NcError::Usage(_)));
        // Empty region is also rejected (defense-in-depth; cli.rs rejects it too).
        assert!(matches!(
            estimate(&img, &FilmBaseSource::Region([0, 0, 0, 4])).unwrap_err(),
            NcError::Usage(_)
        ));
    }

    #[test]
    fn non_finite_samples_never_become_the_base() {
        // A NaN in the sampled region must be excluded from the rank, not returned
        // as the base (a NaN/inf Dmin would poison the density divide downstream).
        let mut img = solid(10, 10, [0.5, 0.5, 0.5]);
        set_px(&mut img, 0, 0, [f32::NAN, f32::INFINITY, f32::NEG_INFINITY]);
        let est = estimate(&img, &FilmBaseSource::Region([0, 0, 10, 10])).unwrap();
        let base = est.base;
        assert!(base.r.is_finite() && base.g.is_finite() && base.b.is_finite());
        assert_close(base, [0.5, 0.5, 0.5], 1e-6);
    }

    #[test]
    fn percentile_is_rounded_rank_over_finite_values() {
        // round((4-1)*0.5) = round(1.5) = 2 → the 3rd finite value (0.3), no
        // interpolation; non-finite values are excluded from the rank.
        let mut v = vec![f32::NAN, 0.1, 0.2, 0.3, 0.4, f32::INFINITY];
        assert_eq!(percentile(&mut v, 0.5), 0.3);
        let mut empty: Vec<f32> = vec![f32::NAN];
        assert_eq!(percentile(&mut empty, 0.5), 0.0);
    }

    #[test]
    fn degenerate_region_base_errors_loudly() {
        // A `--base-region` on the dark holder yields a zero channel; `estimate`
        // must reject it at birth (not print a poison Dmin `hanten measure-base` would
        // echo back), naming a recovery flag.
        let mut img = solid(50, 50, [0.4, 0.3, 0.2]);
        fill_rect(&mut img, [0, 0, 10, 10], [0.0, 0.0, 0.0]);
        let err = estimate(&img, &FilmBaseSource::Region([0, 0, 10, 10])).unwrap_err();
        assert!(matches!(err, NcError::Other(_)), "got {err:?}");
        assert!(
            err.to_string().contains("--film-base"),
            "degenerate-base error must name a recovery flag: {err}"
        );
    }

    // --- The effective-area measurement ------------------------------------------

    /// A deterministic pseudo-random stream in `[0, 1)` (an LCG), so noise fixtures
    /// need no dependency and never change.
    fn lcg(seed: &mut u64) -> f32 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A sample as the decoder produces it: a 16-bit code over 65535.
    fn decoded(v: f32) -> f32 {
        (v.clamp(0.0, 1.0) * 65535.0).round() / 65535.0
    }

    /// One population of unexposed film: `base` with ±4% grain on every channel,
    /// quantized to 16-bit codes as a decoded scan is.
    fn grainy_film(w: u32, h: u32, base: [f32; 3]) -> LinearImage {
        let mut seed = 42;
        let mut buf = Vec::with_capacity((w * h * 3) as usize);
        for _ in 0..w * h {
            for b in base {
                buf.push(decoded(b * (0.96 + 0.08 * lcg(&mut seed))));
            }
        }
        LinearImage::new(w, h, buf, None).unwrap()
    }

    /// The per-channel `p`-quantile over `rect`, by sorting — the reference the
    /// histogram must reproduce.
    fn sorted_percentile(img: &LinearImage, rect: [u32; 4], p: f32) -> [f32; 3] {
        let mut chans = region_channels(img, rect).unwrap();
        chans.each_mut().map(|c| percentile(c, p))
    }

    #[test]
    fn the_histogram_reproduces_the_sorted_percentile_on_decoded_data() {
        let img = grainy_film(64, 48, [0.53, 0.26, 0.16]);
        let rect = [5, 3, 50, 40];
        let hist = CodeHistogram::of(&img, rect).unwrap();
        for p in [0.0, 0.1, 0.5, 0.9, 0.97, 1.0] {
            let want = sorted_percentile(&img, rect, p);
            for (c, want) in want.iter().enumerate() {
                assert_eq!(
                    hist.percentile(c, p).to_bits(),
                    want.to_bits(),
                    "p {p} c {c}"
                );
            }
        }
    }

    #[test]
    fn the_area_is_read_at_its_median_not_its_bright_tail() {
        // One population: an extreme percentile lands in the grain's bright tail,
        // an understated density. The measurement takes the median.
        let img = grainy_film(200, 150, [0.53, 0.26, 0.16]);
        let area = effective_area(&img, 0.05).unwrap();
        let est = measure_area(&img, &area).unwrap();
        assert_eq!(est.percentile, Some(AREA_PERCENTILE));
        assert!(est.warnings.is_empty(), "{:?}", est.warnings);
        let median = sorted_percentile(&img, area.region, 0.5);
        assert_eq!(<[f32; 3]>::from(est.base), median);
        let p97 = sorted_percentile(&img, area.region, SAMPLE_PERCENTILE);
        assert!(
            median.iter().zip(p97).all(|(m, p)| *m < p),
            "{median:?} vs {p97:?}"
        );
    }

    #[test]
    fn a_masked_frame_measures_the_same_base_as_the_frame_cropped_by_hand() {
        // The holder contributes nothing: painting it the most extreme value on
        // either side leaves the base unmoved, and the base equals the median of
        // the region alone — the frame as if its holder had been cropped away.
        let film = grainy_film(200, 160, [0.53, 0.26, 0.16]);
        let with_holder = |rgb: [f32; 3]| {
            let mut img = film.clone();
            img.ir = ir_holder_edges(200, 160, [12, 8, 20, 6]).ir;
            img.ir_verified = true;
            for rect in [
                [0, 0, 200, 12],
                [0, 152, 200, 8],
                [0, 0, 20, 160],
                [194, 0, 6, 160],
            ] {
                fill_rect(&mut img, rect, rgb);
            }
            img
        };
        let dark = with_holder([0.0; 3]);
        let bright = with_holder([1.0; 3]);
        let area = effective_area(&dark, 0.0).unwrap();
        assert!(area.holder_applied, "{area:?}");
        assert_eq!(area, effective_area(&bright, 0.0).unwrap());

        let base = |img| <[f32; 3]>::from(measure_area(img, &area).unwrap().base);
        assert_eq!(base(&dark), base(&bright));
        assert_eq!(base(&dark), sorted_percentile(&film, area.region, 0.5));
    }

    #[test]
    fn a_holder_deeper_on_one_edge_is_cut_on_that_edge_only() {
        // Per edge, not the worst edge everywhere: the left holder is five times
        // the right one, and only the left cut is deep.
        let mut img = grainy_film(300, 200, [0.53, 0.26, 0.16]);
        img.ir = ir_holder_edges(300, 200, [6, 6, 30, 6]).ir;
        img.ir_verified = true;
        let area = effective_area(&img, 0.0).unwrap();
        let holder = area.holder.expect("the holder was measured");
        let [x, _, w, _] = area.region;
        let right_margin = 300 - x - w;
        assert!(holder.left >= 30 && holder.right < 30, "{holder:?}");
        assert!(x > right_margin, "left cut {x} vs right cut {right_margin}");

        // And the base is read over exactly that per-edge rectangle.
        let est = measure_area(&img, &area).unwrap();
        assert_eq!(
            <[f32; 3]>::from(est.base),
            sorted_percentile(&img, area.region, 0.5)
        );
    }

    #[test]
    fn a_picture_frame_warns_that_the_area_is_not_unexposed_film() {
        // The median of a picture is a plausible, wrong base; the spread is what
        // says so.
        let (w, h) = (120u32, 80u32);
        let mut buf = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let t = (x + y) as f32 / (w + h) as f32;
                buf.extend_from_slice(&[0.05 + 0.35 * t, 0.03 + 0.20 * t, 0.02 + 0.10 * t]);
            }
        }
        let img = LinearImage::new(w, h, buf, None).unwrap();
        let est = measure_area(&img, &effective_area(&img, 0.05).unwrap()).unwrap();
        assert!(
            est.warnings.iter().any(|w| w.contains("not uniform")),
            "{:?}",
            est.warnings
        );
    }

    #[test]
    fn a_dark_area_errors_loudly_naming_a_remedy() {
        let img = solid(50, 50, [0.4, 0.0, 0.2]);
        let err = measure_area(&img, &effective_area(&img, 0.05).unwrap()).unwrap_err();
        assert!(matches!(err, NcError::Other(_)), "{err:?}");
        assert!(err.to_string().contains("--base-region"), "{err}");
    }

    // --- The holder march and the effective area ------------------------------

    /// Attach a flat, **marker-verified** IR plane of value `v` (the march consumes
    /// only a verified plane).
    fn with_uniform_ir(mut img: LinearImage, v: f32) -> LinearImage {
        img.ir = Some(vec![v; (img.width * img.height) as usize]);
        img.ir_verified = true;
        img
    }

    /// Set one IR pixel in place (the image must already carry an IR plane).
    fn set_ir(img: &mut LinearImage, x: u32, y: u32, v: f32) {
        let i = (y * img.width + x) as usize;
        img.ir.as_mut().expect("image has an IR plane")[i] = v;
    }

    /// Fill a rectangle of the IR plane with `v`.
    fn fill_ir_rect(img: &mut LinearImage, [x, y, w, h]: [u32; 4], v: f32) {
        for yy in y..y + h {
            for xx in x..x + w {
                set_ir(img, xx, yy, v);
            }
        }
    }

    /// IR transmission of film vs the opaque holder on a scan whose film is
    /// IR-transparent (measured ≈ 0.6 vs ≈ 0.02 — see
    /// [`IR_HOLDER_MAX_TRANSMISSION`]).
    const IR_FILM: f32 = 0.6;
    const IR_HOLDER: f32 = 0.02;
    /// A frame whose *film* is itself opaque to IR: the Ilford HP5 fully-exposed
    /// leader, interior median 0.0165 (`ir-usability-detection`). Indistinguishable
    /// from the holder, which is what the usability verdict exists to catch.
    const IR_OPAQUE_FILM: f32 = 0.0165;

    // --- the effective measurement area (`holder-depth-mask`) ----------------

    /// A frame whose film is IR-transparent, wrapped by an IR-dark holder of the
    /// given per-edge depths. Corners overlap, as a real holder ring's do — which
    /// is the case the depth march has to survive.
    fn ir_holder_edges(w: u32, h: u32, [top, bottom, left, right]: [u32; 4]) -> LinearImage {
        let mut img = with_uniform_ir(solid(w, h, [0.2, 0.2, 0.2]), IR_FILM);
        for (depth, rect) in [
            (top, [0, 0, w, top]),
            (bottom, [0, h - bottom, w, bottom]),
            (left, [0, 0, left, h]),
            (right, [w - right, 0, right, h]),
        ] {
            if depth > 0 {
                fill_ir_rect(&mut img, rect, IR_HOLDER);
            }
        }
        img
    }

    #[test]
    fn holder_depths_measures_each_edge_independently() {
        // 200x200 → probe step 2, so a depth is resolved to within one step. The
        // four edges differ deliberately: a rectangle taking the worst edge
        // everywhere would report 12 on all four.
        let img = ir_holder_edges(200, 200, [4, 8, 12, 2]);
        let d = holder_depths(&img).expect("IR separates on this frame");
        assert_eq!((d.top, d.bottom, d.left, d.right), (4, 8, 12, 2));
        assert!(!d.capped.any(), "none of these reaches the march cap");
    }

    #[test]
    fn a_holder_wrapping_the_whole_border_is_measured_not_declined() {
        // The normal case (22 of 25 real chromogenic frames): every along-edge
        // segment reads holder at the edge, and the answer is how deep it goes.
        let img = ir_holder_edges(200, 200, [6, 6, 6, 6]);
        let d = holder_depths(&img).expect("measured, not declined");
        assert_eq!((d.top, d.bottom, d.left, d.right), (6, 6, 6, 6));
    }

    #[test]
    fn the_deepest_segment_sets_the_whole_edge() {
        // A holder covering only part of the top edge, deeper than the ring: the
        // rectangle cannot follow the notch, so the whole top edge widens to it.
        let mut img = ir_holder_edges(200, 200, [4, 4, 4, 4]);
        fill_ir_rect(&mut img, [80, 0, 40, 20], IR_HOLDER);
        let d = holder_depths(&img).unwrap();
        assert_eq!(d.top, 20, "the deepest segment wins");
        assert_eq!((d.bottom, d.left, d.right), (4, 4, 4));
    }

    #[test]
    fn holder_depths_declines_when_it_cannot_measure_rather_than_guessing() {
        // No IR plane at all.
        let no_ir = solid(200, 200, [0.2, 0.2, 0.2]);
        assert!(no_ir.ir.is_none());
        assert!(holder_depths(&no_ir).is_none());

        // A plane identified by shape alone: carried, but not trusted for a
        // threshold that moves every measurement downstream.
        let mut shape_only = ir_holder_edges(200, 200, [6, 6, 6, 6]);
        shape_only.ir_verified = false;
        assert!(holder_depths(&shape_only).is_none());

        // This frame's own film is as IR-opaque as the holder (an exposed silver
        // frame). Nothing here can separate the two, so it declines — the normal
        // path for B&W, not an error.
        let opaque = with_uniform_ir(solid(200, 200, [0.2, 0.2, 0.2]), IR_OPAQUE_FILM);
        assert!(opaque.ir_verified);
        assert!(holder_depths(&opaque).is_none());
    }

    #[test]
    fn a_settled_march_says_so_and_an_unsettled_one_reports_the_deeper_pass() {
        // `converged` is the difference between a settled measurement and whichever
        // phase the last pass landed on. The edge-depth map is not monotone (a
        // deeper trim narrows the along-edge segments, so a partially-covered
        // segment can start reading holder), so nothing excludes a cycle — and an
        // unsettled answer takes the deeper of the last two passes, which over-cuts
        // for a two-phase cycle without guaranteeing it for a longer one
        // (`HolderDepths::converged`).
        let settled = holder_depths(&ir_holder_edges(200, 200, [4, 8, 12, 2])).expect("measured");
        assert!(
            settled.converged,
            "a real frame settles — falsifiability for the flag"
        );

        // The merge rule itself, exercised directly: whatever phases the last two
        // passes landed on, the reported depth is their elementwise max.
        assert_eq!(
            merge_unsettled_passes([4, 8, 12, 2], [6, 8, 3, 2]),
            [6, 8, 12, 2]
        );
    }

    #[test]
    fn exhausting_the_passes_merges_the_last_two_rather_than_the_final_phase() {
        // At `HOLDER_MARCH_PASSES` the exhaustion path is unreachable — no fixture
        // cycles — which is why the loop takes `passes` as a parameter. With
        // `passes = 1` any frame carrying a holder reaches it: pass 1 marches
        // untrimmed (the corner-contaminated upper bound) and cannot repeat the
        // all-zero start, so the run ends `!converged` and the merge runs against
        // the previous pass.
        let img = ir_holder_edges(200, 200, [4, 8, 12, 2]);
        let ir = img.ir.as_deref().expect("the fixture carries an IR plane");
        let step = holder_probe_depth(&img);

        let (settled, settled_capped, converged) =
            march_to_fixed_point(&img, ir, step, HOLDER_MARCH_PASSES);
        assert!(
            converged,
            "falsifiability: this frame settles when given passes"
        );
        assert_eq!(settled, [4, 8, 12, 2]);
        assert_eq!(
            settled_capped, [false; 4],
            "and it settles on a *measurement*, not on capped edges — discarding \
             this into `_` let the settled pass cap unnoticed"
        );

        let (one, capped_one, converged_one) = march_to_fixed_point(&img, ir, step, 1);
        assert!(!converged_one, "one pass cannot repeat the all-zero start");
        // Untrimmed, the ring's corners make every edge read holder for the frame's
        // whole extent, so pass 1 runs to the cap on all four edges. That is the
        // over-cut direction the merge rule prefers.
        assert_eq!(
            one, [50; 4],
            "pass 1 is the corner-contaminated upper bound"
        );
        assert_eq!(
            capped_one, [true; 4],
            "and it gets there by capping — on every edge"
        );
        for (i, (&m, &s)) in one.iter().zip(settled.iter()).enumerate() {
            assert!(m >= s, "edge {i}: {m} under-cut the settled {s}");
        }

        // One pass reaches the exhaustion path but cannot *test* it: both merged
        // operations are identities there. `merge(P1, [0; 4])` is `P1`, and
        // `prev_capped` is `[false; 4]` on the all-zero start state, so the
        // elementwise cap merge is a no-op — dropping either from the merge leaves the arm
        // above green. Two passes is the shortest run that pins both, because pass 2
        // already reads the settled depths while pass 1 sits at the cap:
        //
        //   P1 = [50, 50, 50, 50] capped     P2 = [4, 8, 12, 2] not capped
        //
        // so the merge must report P1's depths and P1's `capped`, neither of which
        // pass 2 carries. Don't collapse this back into the single-pass arm.
        let (two, capped_two, converged_two) = march_to_fixed_point(&img, ir, step, 2);
        assert!(
            !converged_two,
            "P2 differs from P1, so two passes do not settle"
        );
        assert_eq!(
            two, one,
            "the merge must take the *previous* pass, not the final one \
             (`last = Some(prev)`); holding the final pass would report P2"
        );
        assert_eq!(
            capped_two, [true; 4],
            "`capped` must carry from pass 1 through the elementwise merge; \
             pass 2 alone does not cap"
        );
    }

    #[test]
    fn a_frame_too_small_to_probe_a_band_is_not_measured_rather_than_capped() {
        // `capped` is documented as a *floor* on the real holder, which is a claim
        // about a band that was read. On a frame where the probe step does not fit
        // inside half an edge, the march loop never runs a single band, so reporting
        // the cap would be a verdict on no evidence. "Not measured" is the honest
        // answer and every caller already handles it.
        let tiny = with_uniform_ir(solid(3, 3, [0.2, 0.2, 0.2]), IR_FILM);
        assert!(holder_probe_depth(&tiny) > tiny.width.min(tiny.height) / 2);
        assert!(holder_depths(&tiny).is_none());

        // Only the inset is left, and the area still reports honestly.
        let area = effective_area(&tiny, 0.0).unwrap();
        assert!(area.holder.is_none() && !area.holder_applied);

        // Falsifiability: one step bigger and the same frame *is* measured.
        let ok = with_uniform_ir(solid(4, 4, [0.2, 0.2, 0.2]), IR_FILM);
        assert!(holder_depths(&ok).is_some());
    }

    #[test]
    fn holder_applied_separates_a_moved_rectangle_from_a_measured_zero() {
        // The fact callers want is "did consuming the IR plane change anything?",
        // and it is returned rather than re-derived from the inputs.
        // All-zero depths are a *measurement* that moved nothing, which is what both
        // committed fixtures and the 2026-09 rolls read.
        let moved = effective_area(&ir_holder_edges(200, 200, [4, 8, 12, 2]), 0.05).unwrap();
        assert!(moved.holder.is_some() && moved.holder_applied);

        let cropped =
            effective_area(&with_uniform_ir(solid(200, 200, [0.2; 3]), IR_FILM), 0.05).unwrap();
        assert!(
            cropped.holder.is_some() && !cropped.holder_applied,
            "measured, no holder — the plane was read and moved nothing"
        );

        let not_measured = effective_area(&solid(200, 200, [0.2; 3]), 0.05).unwrap();
        assert!(not_measured.holder.is_none() && !not_measured.holder_applied);
    }

    #[test]
    fn an_already_cropped_frame_measures_zero_rather_than_declining() {
        // No holder anywhere: the answer is "measured, and there is none", which is
        // a different report from "not measured". Both cut to the same region here;
        // only the provenance tells them apart.
        let cropped = with_uniform_ir(solid(200, 200, [0.2, 0.2, 0.2]), IR_FILM);
        let d = holder_depths(&cropped).expect("measured");
        assert_eq!((d.top, d.bottom, d.left, d.right), (0, 0, 0, 0));

        let measured = effective_area(&cropped, 0.05).unwrap();
        assert!(measured.holder.is_some(), "measured, no holder");
        let not_measured = effective_area(&solid(200, 200, [0.2; 3]), 0.05).unwrap();
        assert!(not_measured.holder.is_none(), "not measured at all");
        assert_eq!(
            measured.region, not_measured.region,
            "same region, different verdict — which is why the region alone cannot \
             be the report"
        );
    }

    #[test]
    fn a_band_deeper_than_the_march_cap_reports_the_cap_and_says_so() {
        // 200x200 → cap 50. A 60 px IR-dark left band is deeper than any holder, so
        // the march stops at the cap and the flag says the number is not a
        // measurement. The 10 px beyond the cap keeps top and bottom reading dark
        // at every depth, so they cap too: an over-cut, which is the only
        // direction a cap can err in (`HOLDER_MARCH_MAX_FRAC`).
        let img = ir_holder_edges(200, 200, [4, 4, 60, 4]);
        let d = holder_depths(&img).unwrap();
        assert_eq!((d.top, d.bottom, d.left, d.right), (50, 50, 50, 4));
        assert_eq!(
            (d.capped.top, d.capped.bottom, d.capped.left, d.capped.right),
            (true, true, true, false),
            "every edge reporting the cap is flagged, the inflated ones included"
        );
        let warnings = effective_area_warnings(&effective_area(&img, 0.05).unwrap());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("hit its cap") && warnings[0].contains("left"),
            "the count alone would pass on the *unsettled* warning instead: {}",
            warnings[0]
        );

        let shallow = ir_holder_edges(200, 200, [4, 4, 4, 4]);
        assert!(
            !holder_depths(&shallow).unwrap().capped.any(),
            "falsifiability: `capped` is not simply always set"
        );
        assert!(
            effective_area_warnings(&effective_area(&shallow, 0.05).unwrap()).is_empty(),
            "falsifiability: an uncapped settled frame warns about nothing"
        );
    }

    #[test]
    fn a_capped_edge_over_cuts_its_perpendicular_edges_and_says_so() {
        // The cap is the only route by which one edge moves another: a *mid-edge*
        // notch cannot (left/right sample only `x in [0, left)` and `[w-right, w)`),
        // and a deep-but-uncapped edge produces a trim that exactly covers its own
        // corner.
        //
        // 400x400 → cap 100. A 120 px IR-dark top band is deeper than any holder:
        // the top reports the cap, the 20 px beyond it stays inside the left/right
        // edges' along-edge extent, and they cap too — at a *stable* fixed point,
        // so this comes back `converged: true`. Their 10 px holder reports as 100.
        let img = ir_holder_edges(400, 400, [120, 10, 10, 10]);
        let d = holder_depths(&img).expect("IR separates on this frame");
        assert_eq!((d.top, d.bottom, d.left, d.right), (100, 10, 100, 100));
        assert!(
            d.converged,
            "the cap creates a *stable* fixed point — which is why `converged` \
             cannot be read on its own"
        );
        assert_eq!(
            (d.capped.top, d.capped.bottom, d.capped.left, d.capped.right),
            (true, false, true, true),
            "the inflated edges cap too, which is what makes them visible"
        );

        let warnings = effective_area_warnings(&effective_area(&img, 0.05).unwrap());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        for expect in [
            "top, left, right",
            "not a measurement",
            "only over-cuts",
            "--measure-inset",
        ] {
            assert!(
                warnings[0].contains(expect),
                "the warning must name the edges, the error's direction and the \
                 remedy, missing {expect:?}: {}",
                warnings[0]
            );
        }

        // Falsifiability: a band exactly at the cap flags the top alone and leaves
        // the other three measured.
        let at_cap = holder_depths(&ir_holder_edges(400, 400, [100, 10, 10, 10])).unwrap();
        assert_eq!(
            (at_cap.top, at_cap.bottom, at_cap.left, at_cap.right),
            (100, 10, 10, 10)
        );
        assert_eq!(
            (
                at_cap.capped.top,
                at_cap.capped.bottom,
                at_cap.capped.left,
                at_cap.capped.right
            ),
            (true, false, false, false)
        );
    }

    #[test]
    fn an_unsettled_march_warns_that_the_rectangle_is_not_a_measurement() {
        // `converged: false` is unreachable on any fixture at `HOLDER_MARCH_PASSES`,
        // so the warning is pinned on a hand-built `HolderDepths` — the flags are
        // what `effective_area_warnings` reads, and both reach `--strict`.
        let unsettled = EffectiveArea {
            region: [10, 10, 180, 180],
            holder: Some(HolderDepths {
                top: 4,
                bottom: 4,
                left: 4,
                right: 4,
                capped: CappedEdges::default(),
                converged: false,
            }),
            holder_applied: true,
            inset: 10,
        };
        let warnings = effective_area_warnings(&unsettled);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("did not settle"), "{}", warnings[0]);
        // And it must not promise an over-cut. The merge guarantees that direction
        // only for a two-phase cycle or a march still settling downward; a longer
        // cycle or an upward transient can sit below the fixed point and leave
        // holder *inside* the region, which is the opposite of what a reader would
        // act on. The honest wording is `HolderDepths::converged`'s.
        assert!(
            warnings[0].contains("leave holder inside the region"),
            "the warning must name the under-cut case too: {}",
            warnings[0]
        );
        assert!(
            !warnings[0].contains("rather than leaving holder inside the region"),
            "the unconditional over-cut claim is back: {}",
            warnings[0]
        );

        // Not measured at all: nothing to warn about, since no depth is claimed.
        let not_measured = EffectiveArea {
            holder: None,
            holder_applied: false,
            ..unsettled
        };
        assert!(effective_area_warnings(&not_measured).is_empty());
    }

    #[test]
    fn the_inset_is_a_fraction_of_the_original_frame_not_the_remainder() {
        // Same frame size, very different holder depths: the inset is identical, so
        // a stated fraction does not move with an IR measurement.
        let shallow = effective_area(&ir_holder_edges(200, 200, [2, 2, 2, 2]), 0.05).unwrap();
        let deep = effective_area(&ir_holder_edges(200, 200, [30, 30, 30, 30]), 0.05).unwrap();
        assert_eq!(shallow.inset, 10, "5% of the 200 px short edge");
        assert_eq!(deep.inset, shallow.inset);

        // And the two cuts compose in order: holder first, then the inset.
        assert_eq!(shallow.region, [12, 12, 176, 176]);
        assert_eq!(deep.region, [40, 40, 120, 120]);
    }

    #[test]
    fn the_inset_runs_even_when_the_holder_was_not_measured() {
        // The inset is not a fallback for a missing holder cut — it is the second
        // cut, and where the first did not run it is simply the only one.
        let no_ir = solid(200, 200, [0.2, 0.2, 0.2]);
        let area = effective_area(&no_ir, 0.05).unwrap();
        assert!(area.holder.is_none());
        assert_eq!(area.region, [10, 10, 180, 180]);
    }

    #[test]
    fn a_measured_frame_floors_the_inset_at_one_probe_step() {
        // 200x200 → probe step 2, so a **3 px** top holder puts the boundary inside
        // the second band (rows 2..4: one holder row, one film row). That band's
        // median reads film, so the march reports 2 and row 2 stays inboard of the
        // reported depth — the march can only ever resolve a depth to the start of
        // the first film-reading band. At `--measure-inset 0` the floor is the only
        // thing keeping that residual holder out of the region.
        let img = ir_holder_edges(200, 200, [3, 0, 0, 0]);
        let d = holder_depths(&img).expect("measured");
        assert_eq!(
            d.top, 2,
            "the march under-reports the 3 px holder by design"
        );

        let area = effective_area(&img, 0.0).unwrap();
        assert_eq!(area.inset, 2, "a stated 0 is floored at the probe step");
        let ir = img.ir.as_deref().expect("the fixture carries an IR plane");
        let [x, y, w, h] = area.region;
        let holder_px = (y..y + h)
            .flat_map(|row| (x..x + w).map(move |col| (row, col)))
            .filter(|&(row, col)| {
                ir[row as usize * 200 + col as usize] <= IR_HOLDER_MAX_TRANSMISSION
            })
            .count();
        assert_eq!(
            holder_px, 0,
            "without the floor the region starts at y = 2 and keeps a holder row in"
        );

        // It binds only near zero: the default is well above one step.
        assert_eq!(effective_area(&img, 0.05).unwrap().inset, 10);

        // A measured **zero** is floored too, not only a moved rectangle: "no holder
        // in the outermost band" is not "no holder", since the band's median can
        // hide up to half a band of it. So the floor is keyed on the march having
        // *run*, not on `holder_applied`.
        let cropped = effective_area(&with_uniform_ir(solid(200, 200, [0.2; 3]), IR_FILM), 0.0)
            .expect("measured, no holder");
        assert!(cropped.holder.is_some() && !cropped.holder_applied);
        assert_eq!(cropped.inset, 2);

        // Where the holder was *not* measured there is no measurement resolution to
        // respect, and a stated 0 is exact.
        assert_eq!(
            effective_area(&solid(200, 200, [0.2; 3]), 0.0)
                .unwrap()
                .inset,
            0
        );
    }

    #[test]
    fn effective_area_refuses_an_empty_region_and_a_nonsense_fraction() {
        // 40 px of holder each side (still shallow enough that the interior
        // usability verdict passes) plus a 30% inset is 100 px from each side of a
        // 200 px frame — exactly nothing left.
        let img = ir_holder_edges(200, 200, [40, 40, 40, 40]);
        assert!(
            holder_depths(&img).is_some_and(|d| d.left == 40),
            "the fixture must actually measure a holder, or this tests the wrong path"
        );
        let err = effective_area(&img, 0.3).unwrap_err();
        assert!(
            format!("{err}").contains("empty"),
            "an empty measurement region must be loud: {err}"
        );
        for bad in [-0.1, 0.5, 1.0, f32::NAN] {
            assert!(
                effective_area(&img, bad).is_err(),
                "inset fraction {bad} must be refused"
            );
        }
    }

    #[test]
    fn the_effective_area_never_reports_a_region_outside_the_frame() {
        // The rectangle is what consumers clamp their walk to, so a region running
        // past the frame would index out of bounds in every one of them.
        for depths in [[0, 0, 0, 0], [2, 9, 13, 4], [40, 1, 1, 40]] {
            let img = ir_holder_edges(200, 200, depths);
            let [x, y, w, h] = effective_area(&img, 0.05).unwrap().region;
            assert!(w > 0 && h > 0, "non-empty for {depths:?}");
            assert!(
                x + w <= 200 && y + h <= 200,
                "inside the frame for {depths:?}"
            );
        }
    }

    #[test]
    fn a_narrow_edge_artifact_does_not_drag_the_whole_edge_to_the_cap() {
        // The segments are spread evenly, so the along-edge extent not dividing by
        // `IR_HOLDER_SEGMENTS` cannot leave a sliver segment too narrow to measure.
        // Measured on Portra160 `1102` before the fix: a 6 px trailing segment
        // never cleared and reported the left holder as the 900 px cap, where the
        // other 24 segments agreed on 108-126 px — 15% of the frame width thrown
        // away, with `capped` the only signal.
        //
        // 203 px tall so the left edge's trimmed extent (195) is not a multiple of
        // 24, plus a deep IR-dark streak at the far end of that edge, narrow enough
        // to be a minority of its own segment.
        let mut img = ir_holder_edges(200, 203, [4, 4, 4, 4]);
        fill_ir_rect(&mut img, [0, 196, 60, 3], IR_HOLDER);

        let d = holder_depths(&img).expect("IR separates");
        assert!(
            !d.capped.any(),
            "a narrow artifact must not cap the edge; got {d:?}"
        );
        assert!(
            d.left <= 8,
            "the left holder is 4 px, not the march cap; got {}",
            d.left
        );
    }

    #[test]
    fn ir_separability_reads_the_film_not_the_border() {
        // The holder occludes the frame edge, so a verdict about the film must
        // sample the interior. A frame with a dark border and transparent film is
        // usable -- if the border were included it could drag the median under the
        // bar and refuse exactly the unexposed reference frame `Dmin` comes from.
        let mut img = with_uniform_ir(solid(100, 100, [0.2, 0.1, 0.05]), IR_FILM);
        for rect in [
            [0, 0, 100, 8],
            [0, 92, 100, 8],
            [0, 0, 8, 100],
            [92, 0, 8, 100],
        ] {
            fill_ir_rect(&mut img, rect, IR_HOLDER);
        }
        let sep = ir_separability(&img).expect("an IR plane is present");
        assert!(
            sep.usable,
            "interior median {} must clear the bar",
            sep.interior_median
        );
        assert!((sep.interior_median - IR_FILM).abs() < 1e-6);

        // No IR plane → no verdict (distinct from "measured and unusable").
        assert!(ir_separability(&solid(100, 100, [0.2, 0.1, 0.05])).is_none());
    }

    #[test]
    fn ir_separability_reproduces_the_measured_frame_verdicts() {
        // The four Ilford HP5 frames the task recorded, by interior median: the
        // verdict tracks the *frame's* density, not the stock's chemistry -- all
        // four are the same silver-halide roll.
        for (interior, usable, what) in [
            (0.4730, true, "1364 unexposed"),
            (0.4602, true, "1330 half-leader"),
            (0.0748, false, "1354 regular"),
            (0.0165, false, "1329 leader"),
            // The lowest chromogenic frame measured across 9 rolls, leaders
            // included -- the margin that keeps the demotion from regressing the
            // scans the mask already served.
            (0.5760, true, "chromogenic minimum"),
        ] {
            let img = with_uniform_ir(solid(60, 60, [0.2, 0.1, 0.05]), interior);
            let sep = ir_separability(&img).expect("an IR plane is present");
            assert_eq!(sep.usable, usable, "{what} (interior {interior})");
        }

        // The threshold itself, from both sides: a bare epsilon decides it, so the
        // constant is what these verdicts rest on and nothing else.
        let below = with_uniform_ir(solid(60, 60, [0.2; 3]), IR_USABLE_MIN_INTERIOR - 1e-4);
        let at = with_uniform_ir(solid(60, 60, [0.2; 3]), IR_USABLE_MIN_INTERIOR);
        assert!(!ir_separability(&below).unwrap().usable);
        assert!(ir_separability(&at).unwrap().usable);
    }
}

/// Probe: [`share_above_base`] on every manifest roll's picture frames, against the
/// base measured from its unexposed frame. Prints derived numbers only.
#[cfg(test)]
mod polarity_probe {
    use super::*;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires ../nc-assets; run with --ignored --nocapture"]
    fn share_above_base_on_real_rolls() {
        let root = std::env::var("NC_ASSETS")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../nc-assets"));
        let Ok(text) = std::fs::read_to_string(root.join("manifest.json")) else {
            eprintln!("SKIP: no manifest");
            return;
        };
        let m: serde_json::Value = serde_json::from_str(&text).unwrap();
        for (roll, entry) in m["rolls"].as_object().unwrap() {
            let frames = entry["frames"].as_array().unwrap();
            let of = |role: &str| -> Vec<PathBuf> {
                frames
                    .iter()
                    .filter(|f| f["role"] == role)
                    .map(|f| root.join(f["file"].as_str().unwrap()))
                    .collect()
            };
            let base = match of("unexposed").first() {
                Some(p) => {
                    let (img, _) = crate::io::decode::decode_within(p, u64::MAX).unwrap();
                    measure_area(
                        &img,
                        &effective_area(&img, crate::types::DEFAULT_MEASURE_INSET).unwrap(),
                    )
                    .unwrap()
                    .base
                }
                None => {
                    println!("{roll}: no unexposed frame, skipped");
                    continue;
                }
            };
            println!(
                "\n=== {roll} base ({:.4}, {:.4}, {:.4})",
                base.r, base.g, base.b
            );
            for p in of("real") {
                let (img, _) = crate::io::decode::decode_within(&p, u64::MAX).unwrap();
                let region = effective_area(&img, crate::types::DEFAULT_MEASURE_INSET)
                    .unwrap()
                    .region;
                let s: Vec<String> = [1.0f32, 1.1, 1.25, 1.5]
                    .iter()
                    .map(|&r| {
                        let v = share_above_base(&img, region, &base, r).unwrap();
                        format!("x{r}: {:.4}/{:.4}/{:.4}", v[0], v[1], v[2])
                    })
                    .collect();
                println!(
                    "{}  {}",
                    p.file_name().unwrap().to_string_lossy(),
                    s.join("  ")
                );
            }
        }
    }
}
