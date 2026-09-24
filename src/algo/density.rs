//! `density` — density-domain reconstruction (Cineon / negadoctor style), the
//! default, plus the exponential density curve. Density reconstruction and the
//! density curve are **separate** sub-stages, and print rendering happens later,
//! past the ACEScg boundary (core fidelity rule from design-spec §3/§7.2):
//!
//! ## Model (per channel `c`)
//!
//! ```text
//! 1. transmission → density:   D_c  = -log10(max(scan_c, EPS) / base_c)
//! 2. density correction:       B_c  = scale_c · D_c + offset_c
//!    regional balance:         D̄    = mean(B_r, B_g, B_b)   (scalar tone)
//!                              D'_c = B_c + shadow_balance_c · w_lo(D̄)
//!                                         + highlight_balance_c · w_hi(D̄)
//! 3. density curve:            exponential lin_c = 10^(gamma · (D'_c − A))
//!                              (or a stock's characteristic curve — `algo::film_stock`)
//!                              → FilmRgbImage
//! ```
//!
//! Stages 1–2 are [`to_density`] + [`regional_balance`] — **density
//! reconstruction**, owned by [`reconstruct`], which then applies the tagged
//! curve ([`apply_curve`], stage 3) to produce the typed
//! [`FilmRgbImage`] boundary. The print controls run downstream of it, in
//! `pipeline::render_split`, after the NC film RGB v1 → ACEScg mapping.
//!
//! **Regional (shadow/highlight) color balance.** A color *crossover* — a cast
//! that differs between shadows and highlights — is, in density space, a
//! per-channel offset that varies with tone. `regional_balance` adds
//! density-weighted per-channel offsets inside stage 2: `w_lo`/`w_hi` are
//! complementary smoothstep ramps over the corrected-density range `[lo, hi]`
//! (`w_lo = 1` at `lo` fading to `0` at `hi`; `w_hi = 1 − w_lo`), so equal
//! shadow and highlight balances degenerate to a uniform `offset`. The
//! ramps take the **scalar** per-pixel tone `D̄` (the mean of the pre-regional
//! corrected channels), never each channel's own density — per-channel
//! weighting would let one channel of a crossover pixel receive the shadow
//! correction while another receives the highlight one, misfiring on exactly
//! the pixels this control exists to fix. Naming is from the **positive's**
//! point of view: low density (near base) is a scene/positive *shadow*, high
//! density a *highlight* — and positive polarity (below) means a positive
//! balance value brightens that channel in its region. The range anchors come
//! from [`BalanceRange`]: `Auto` measures robust percentiles of the per-pixel
//! tone in a deterministic two-pass within stage 2; `Explicit` short-circuits
//! the measuring pass for roll reuse. The neutral `[0,0,0]` defaults skip the
//! pass entirely — bit-exact with the unbalanced output.
//!
//! **Anchor — owned by the curve stage.** The exponential renders density
//! relative to an anchor `A`: `10^(γ·(D′ − A))`, so `D′ = A` maps to `1.0`. `A`
//! comes from [`AnchorPlacement`], whose one rule pins mid-grey a stated density
//! above the film base. It reads nothing off the frame and no roll reference
//! density, so the render carries no leader's roll-to-roll error, and darker
//! frames render darker (faithful relative exposure).
//!
//! **Polarity.** With `D = -log10(scan/base)` the density is `≥ 0` and *grows*
//! with the film's optical density: the unexposed base (scene black) sits at
//! `D = 0`, and a dense negative area (a scene highlight) has a large `D`. A
//! true positive must get *brighter* as `D` grows, so stage 3 uses
//! `10^(+γ·D')`. This matches darktable `negadoctor`, whose print output
//! increases with film density (verified against its source: denser negative →
//! brighter print).
//!
//! Output is linear, and nothing is clamped here — the encode stage counts and
//! reports any out-of-range samples.

use rayon::prelude::*;

use crate::algo::{FilmRgbImage, ReconstructionReport};
#[cfg(doc)]
use crate::types::AnchorPlacement;
use crate::types::{
    BalanceRange, DensityCurve, DensityParams, FilmBase, LinearImage, NcError, Result,
};

/// Floor applied to the scan transmission before the `log10`, so a zero / negative
/// / denormal sample can't produce `-inf`/`NaN` density (design "fail loudly, never
/// a quietly wrong image" — a dead pixel becomes a very high but finite density
/// rather than poisoning the channel). `1e-6` ≈ −20 stops below unity: darker than
/// any real detail, yet leaves ample headroom before `10^(γ·D)` can overflow `f32`.
///
/// `pub(crate)` rather than `pub(super)` only so `pipeline::stages::golden` can recompute
/// stage 1 in `f64` when it measures how close the shipped `log10` lands to an f32
/// rounding boundary; nothing outside `algo` consumes it at runtime.
pub(crate) const SCAN_EPSILON: f32 = 1e-6;

/// Corrected per-pixel film density `D'` (interleaved RGB), the boundary between
/// the reconstruction sub-stages: the output of [`to_density`] +
/// [`regional_balance`] (stages 1–2) and the input to the density curve
/// ([`apply_curve`], stage 3). The IR plane is carried through untouched
/// (Step-1 rule: preserve, don't consume).
///
/// Algo-internal (`pub(crate)`), not a cross-stage contract type — the typed
/// cross-stage boundary is [`FilmRgbImage`], which only the curve stage mints.
/// It has no validated constructor; its length invariants
/// (`density.len() == w*h*3`, `ir.len() == w*h`) hold by construction because
/// [`to_density`], its only producer, derives them from a validated
/// [`LinearImage`].
#[derive(Clone, Debug)]
pub(crate) struct DensityImage {
    pub width: u32,
    pub height: u32,
    /// Corrected density `D'`, interleaved `r,g,b, r,g,b, …`, `len == w*h*3`.
    pub density: Vec<f32>,
    /// Carried-through IR plane (HDRi input), `len == w*h` when present.
    pub ir: Option<Vec<f32>>,
}

/// Density reconstruction + the tagged curve (stages 1–3, design-spec §7.2):
/// Dmin-normalize into corrected density `D′`, apply the regional balance,
/// place the curve's anchor, then map `D′` through the selected
/// curve into the typed [`FilmRgbImage`]. Pure; the print controls are
/// deliberately **not** here — they run past the ACEScg boundary
/// (`pipeline::render_split`).
pub(super) fn reconstruct(
    image: &LinearImage,
    base: &FilmBase,
    params: &DensityParams,
    curve: &DensityCurve,
) -> Result<(FilmRgbImage, ReconstructionReport)> {
    // `to_density` divides by the per-channel base, so a zero / negative /
    // non-finite base would yield a silently-black or non-finite image.
    // `film_base::estimate` guards every source at birth; this is defense-in-depth
    // for a base reaching the algorithm by another route. Fail loudly instead.
    check_base(base)?;
    let mut density = to_density(image, base, params);

    let balance_range = regional_balance(&mut density, params)?;

    let mut characteristic_out_of_table = None;
    let (film, curve_anchor) = match curve {
        DensityCurve::Exponential(exp) => {
            // The anchor is applied **in the exponent** — `10^(γ·(D' − A))` — not as a
            // separate `10^(−γ·A)` gain: mathematically equivalent, but the factored
            // form overflows `f32` when `γ·D'` alone exceeds the pow10 range even
            // though the anchored exponent is small (e.g. `γ = 5`, EPS-clamped
            // `D' ≈ 8`), turning white into `inf` instead of `1.0`.
            let gamma = exp.gamma;
            let anchor = exp.anchor.anchor(gamma);
            // Defense in depth. Two ways the exponent goes non-finite, and **both**
            // render `10^(−inf) = 0.0` for every sample — an all-black frame that trips
            // neither the clip nor the non-finite counter: the placement's division by
            // the slope can overflow the *anchor* (a positive-but-tiny gamma), and a
            // large-but-finite anchor can overflow the *product* `gamma · anchor`.
            // `validate` rejects both at the CLI boundary, naming the flag; a
            // programmatic caller reaches here first.
            if !anchor.is_finite() || !(gamma * anchor).is_finite() {
                return Err(NcError::Other(format!(
                    "the exponential anchor placement derived a non-usable anchor \
                     ({anchor:e}) at gamma {gamma}: the curve's exponent \
                     `gamma · (density − anchor)` is not finite, so every sample would \
                     render as exactly 0.0. Use a photographic gamma and a smaller anchor \
                     offset"
                )));
            }
            let film = apply_curve(density, move |d| 10f32.powf(gamma * (d - anchor)));
            (film, Some(anchor))
        }
        DensityCurve::Characteristic(ch) => {
            // No anchor to place: the published curve carries it, so the report's
            // `curve_anchor` is `None` rather than a derived number nothing consulted.
            crate::algo::film_stock::check_tables(ch.stock)?;
            let (film, out_of_table) = crate::algo::film_stock::apply_curve(density, ch.stock)?;
            characteristic_out_of_table = Some(out_of_table);
            (film, None)
        }
    };
    Ok((
        film,
        ReconstructionReport {
            curve_anchor,
            balance_range,
            out_of_table: characteristic_out_of_table,
        },
    ))
}

/// Stage 1 + stage 2's per-channel correction — transmission → corrected
/// density (pure). [`regional_balance`] then completes stage 2.
///
/// `D_c = -log10(max(scan_c, EPS) / base_c)` then `D'_c = scale_c·D_c + offset_c`.
/// Dividing by the *per-channel* base is what neutralizes the orange mask: at the
/// base every channel lands on `D = 0`, so an unexposed sample is neutral before
/// any correction; `offset` / `scale` then trim the per-channel density balance
/// and contrast.
///
/// `base` must be finite and `> 0` per channel; [`reconstruct`] enforces this
/// before calling (the CLI validates an explicit base, but an auto/region-estimated
/// base is only checked there), so this stage trusts its inputs and never fails.
///
/// A **non-finite** scan sample (`NaN`/`±inf`) is propagated as `NaN` density rather
/// than laundered by the floor, so `io::encode`'s non-finite counter still surfaces
/// corrupt input downstream. The `SCAN_EPSILON` floor applies only to *finite*
/// zero/negative/denormal transmission (the physically-real dead-pixel case).
pub(crate) fn to_density(
    image: &LinearImage,
    base: &FilmBase,
    params: &DensityParams,
) -> DensityImage {
    let base = [base.r, base.g, base.b];
    let scale = params.scale;
    let offset = params.offset;

    let mut density = vec![0.0f32; image.rgb.len()];
    density
        .par_chunks_exact_mut(3)
        .zip(image.rgb.par_chunks_exact(3))
        .for_each(|(out, px)| {
            for c in 0..3 {
                let s = px[c];
                let d = if s.is_finite() {
                    -(s.max(SCAN_EPSILON) / base[c]).log10()
                } else {
                    f32::NAN
                };
                out[c] = scale[c] * d + offset[c];
            }
        });

    DensityImage {
        width: image.width,
        height: image.height,
        density,
        ir: image.ir.clone(),
    }
}

/// Percentiles of the per-pixel scalar tone (`D̄`) distribution taken as the
/// `Auto` ramp anchors `[lo, hi]`. Symmetric and deliberately robust: the bottom
/// and top half-percent (dust shadows, specular sparkle, hot pixels) are ignored
/// so an outlier can't stretch the ramp and flatten the weights over the real
/// tonal range.
const BALANCE_LO_PERCENTILE: f32 = 0.005;
const BALANCE_HI_PERCENTILE: f32 = 0.995;

/// Cap on how many per-pixel tones the `Auto` range measurement examines:
/// percentiles over ~1M samples are statistically indistinguishable from the full
/// population, and the cap keeps the measuring pass to a small transient buffer.
/// The stride walks whole RGB pixels (chunks of 3), so no channel-bias adjustment
/// is needed.
const BALANCE_MAX_SAMPLES: usize = 1 << 20;

/// Clamped cubic smoothstep: `0` at `t <= 0`, `1` at `t >= 1`, `t²(3 − 2t)`
/// between — smooth (zero slope at both ends) and strictly monotonic inside.
/// The highlight weight `w_hi`; the shadow weight is its complement `1 − w_hi`.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The scalar tone value `D̄` of one interleaved-RGB pixel: the mean of its
/// *finite* corrected densities. A non-finite channel (NaN from corrupt input)
/// is excluded so the two healthy channels still get the right regional
/// correction — the NaN channel itself stays NaN through any offset, so the
/// encoder's non-finite counter still surfaces it. All-non-finite ⇒ `None`.
///
/// The mean itself is also required finite: finite channels of very large
/// magnitude (only reachable via pathological recipe `scale`/`offset`
/// values) can overflow the sum to `±inf`, and an infinite tone would corrupt
/// *both* consumers — as a measured anchor it would make `hi`/`lo` infinite (a
/// flat, silently-wrong ramp for the whole frame), and in the apply pass an
/// `inf/inf` weight ratio would turn into NaN that poisons the pixel's finite
/// channels. Such a pixel is skipped instead. Note the encoder is a reliable
/// backstop only in the positive direction: a hugely *positive* density blows up
/// to non-finite in the curve's `10^(γ·D')` (counted), but a hugely *negative*
/// one underflows to a finite `+0.0` — a quietly black pixel the counters won't
/// flag. Both require absurd (validation-passing but physically-impossible)
/// recipe params to reach, so the skip is a defensive floor, not a routine path.
fn pixel_tone(px: &[f32]) -> Option<f32> {
    let mut sum = 0.0f32;
    let mut n = 0u32;
    for &v in px {
        if v.is_finite() {
            sum += v;
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    let mean = sum / n as f32;
    mean.is_finite().then_some(mean)
}

/// Measure the `Auto` ramp anchors `[lo, hi]` from the per-pixel scalar tone
/// distribution: `lo` is the [`BALANCE_LO_PERCENTILE`] and `hi` the
/// [`BALANCE_HI_PERCENTILE`] taken by **nearest-rank** — index
/// `round((n − 1) · p)` into the sorted finite tones (not the textbook
/// `ceil(n · p)`; the `n − 1` form pins the endpoints so `p = 0`/`1` map to the
/// min/max). The sample is a deterministic stride over whole RGB pixels, capped
/// at [`BALANCE_MAX_SAMPLES`]. Measuring the *same* `D̄` the ramps consume keeps
/// anchors and inputs in one domain, so non-default `scale`/`offset`
/// can't make them drift apart. Returns `None` when the distribution can't
/// define a usable ramp: no finite tones, `lo == hi` (a uniform frame has no
/// shadow/highlight distinction), or a span `hi − lo` that overflows `f32` (only
/// reachable via pathological anchors — an infinite span would flatten the ramp).
fn measure_balance_range(density: &[f32]) -> Option<[f32; 2]> {
    let pixels = density.len() / 3;
    let stride = pixels.div_ceil(BALANCE_MAX_SAMPLES).max(1);
    let mut tones: Vec<f32> = Vec::with_capacity(pixels.div_ceil(stride));
    tones.extend(
        density
            .as_chunks::<3>()
            .0
            .iter()
            .step_by(stride)
            .filter_map(|px| pixel_tone(px.as_slice())),
    );
    if tones.is_empty() {
        return None;
    }
    let last = tones.len() - 1;
    let rank = |p: f32| (last as f32 * p).round() as usize;
    // Two independent O(n) selections; `select_nth_unstable`'s returned order
    // statistic is independent of tie ordering, so this stays deterministic.
    let (_, lo, _) = tones.select_nth_unstable_by(rank(BALANCE_LO_PERCENTILE), f32::total_cmp);
    let lo = *lo;
    let (_, hi, _) = tones.select_nth_unstable_by(rank(BALANCE_HI_PERCENTILE), f32::total_cmp);
    let hi = *hi;
    (lo < hi && (hi - lo).is_finite()).then_some([lo, hi])
}

/// Stage 2 (second half) — regional (shadow/highlight) color balance (pure).
///
/// Adds the density-weighted per-channel offsets of the module-doc model in
/// place: `D'_c = B_c + shadow_balance_c·w_lo(D̄) + highlight_balance_c·w_hi(D̄)`
/// with `w_hi = smoothstep((D̄ − lo) / (hi − lo))`, `w_lo = 1 − w_hi`, and `D̄`
/// the pixel's scalar tone (see [`pixel_tone`]). Outside `[lo, hi]` the weights
/// saturate (0/1), so an explicit roll range still corrects frames whose tones
/// slightly exceed it.
///
/// Returns the resolved `[lo, hi]` for the JSON report (roll reuse), or `None`
/// when no range was consulted: both balances neutral `[0, 0, 0]` (buffer
/// untouched, measuring pass skipped, output **bit-exact** with the unbalanced
/// render — even `+0.0` would flip `-0.0`), or `shadow == highlight` (equal but
/// non-neutral), which collapses to a tone-independent per-channel offset (see
/// below) and likewise needs no range.
///
/// Fails loudly ([`NcError::Other`]) when a balance was requested but the
/// `Auto` range is degenerate (uniform or all-non-finite frame) — a silently
/// skipped correction would be a quietly wrong image; the error names
/// `--balance-range` as the recovery flag.
pub(crate) fn regional_balance(
    image: &mut DensityImage,
    params: &DensityParams,
) -> Result<Option<[f32; 2]>> {
    let shadow = params.shadow_balance;
    let highlight = params.highlight_balance;
    if shadow == [0.0; 3] && highlight == [0.0; 3] {
        return Ok(None);
    }

    // Equal (but non-neutral) balances collapse to a tone-independent per-channel
    // offset: the ramp weights are complementary (`w_lo + w_hi == 1`), so the
    // per-pixel correction `shadow_c·w_lo + highlight_c·w_hi` reduces to
    // `shadow_c` at every tone and never consults the range (design-spec §7.2:
    // "equal shadow and highlight balances degenerate to a uniform
    // density_offset"). Apply the constant offset directly and report no range —
    // measuring one here would only let a uniform/degenerate frame fail
    // spuriously under `Auto`. An unconditional add is safe: a non-finite density
    // plus a finite offset stays non-finite, so the encoder's counter still fires.
    if shadow == highlight {
        image.density.par_chunks_exact_mut(3).for_each(|px| {
            for c in 0..3 {
                px[c] += shadow[c];
            }
        });
        return Ok(None);
    }

    let [lo, hi] = match params.balance_range {
        BalanceRange::Explicit(range) => range,
        BalanceRange::Auto => measure_balance_range(&image.density).ok_or_else(|| {
            NcError::Other(
                "regional balance: cannot measure a density range on this frame \
                 (uniform or non-finite densities); pass an explicit --balance-range LO,HI"
                    .into(),
            )
        })?,
    };
    // `span` is finite and `> 0`: explicit ranges are CLI/recipe-validated
    // (finite, lo < hi, representable span), and `measure_balance_range` rejects
    // any range whose span isn't finite-and-positive.
    let span = hi - lo;
    debug_assert!(
        span.is_finite() && span > 0.0,
        "ramp span must be finite > 0"
    );

    image.density.par_chunks_exact_mut(3).for_each(|px| {
        // A pixel with no finite tone (all-non-finite, or a mean that overflows
        // f32) is left alone — the encoder's non-finite counter surfaces the
        // underlying fault; see `pixel_tone`.
        let Some(tone) = pixel_tone(px) else { return };
        let w_hi = smoothstep((tone - lo) / span);
        let w_lo = 1.0 - w_hi;
        for c in 0..3 {
            px[c] += shadow[c] * w_lo + highlight[c] * w_hi;
        }
    });
    Ok(Some([lo, hi]))
}

/// Whether [`regional_balance`] will actually **consult** the `[lo, hi]` ramp range
/// for these params — i.e. whether it takes neither of the two short-circuits above.
///
/// Both short-circuits are exactly "the balances are equal": a neutral
/// `[0,0,0]`/`[0,0,0]` pair leaves the buffer untouched, and an equal-but-non-neutral
/// pair collapses to a tone-independent per-channel offset. So one comparison decides
/// it, and it lives here — next to the code it mirrors — because a caller that needs
/// the answer (`cli::validate_output_preset` rejects a frame-local `Auto` range under
/// `--output-preset film-master`, but only when it is genuinely measured) would
/// otherwise silently drift if a short-circuit changed.
///
/// Note this is about the *range*, not the correction: an equal pair still adjusts the
/// image, it just needs no measured anchors.
pub(crate) fn consults_balance_range(params: &DensityParams) -> bool {
    params.shadow_balance != params.highlight_balance
}

/// [`apply_curve`] with a **per-channel** tone function, for a curve whose response
/// differs by dye layer.
///
/// The parametric curves share one function across all three channels; the characteristic
/// curve does not, because the film does not — every C-41 stock measured has a blue layer
/// 12-19% steeper than its red one. Same buffer discipline as [`apply_curve`]: in place,
/// then through the validated constructor.
pub(crate) fn apply_curve_per_channel(
    density: DensityImage,
    tone: impl Fn(usize, f32) -> f32 + Sync,
) -> FilmRgbImage {
    let mut rgb = density.density;
    rgb.par_chunks_exact_mut(3).for_each(|px| {
        for (c, v) in px.iter_mut().enumerate() {
            *v = tone(c, *v);
        }
    });
    FilmRgbImage::from_linear(
        LinearImage::new(density.width, density.height, rgb, density.ir)
            .expect("the curve preserves the validated buffer-length invariants"),
    )
}

/// Stage 3 — apply a density curve `tone` (corrected density → positive
/// linear) to every sample, minting the typed [`FilmRgbImage`] boundary.
///
/// The producer path for the exponential, which applies one function to every
/// channel. It is not the only one: the characteristic curve mints its image through
/// [`apply_curve_per_channel`].
///
/// Pure and unclamped; a non-finite density (or a curve output that overflows) rides
/// through so `io::encode`'s counters surface it.
///
/// Consumes the `DensityImage` (a use-once intermediate): the density buffer is
/// transformed in place and the IR plane is moved, so no image-sized buffer is
/// allocated or cloned here.
pub(crate) fn apply_curve(density: DensityImage, tone: impl Fn(f32) -> f32 + Sync) -> FilmRgbImage {
    let mut rgb = density.density;
    rgb.par_chunks_exact_mut(3).for_each(|px| {
        for v in px.iter_mut() {
            *v = tone(*v);
        }
    });

    // Lengths are inherited unchanged from a `DensityImage` built from a validated
    // `LinearImage`, so the invariants hold by construction. Route through the
    // validated constructor anyway — its checks are O(1) (buffer lengths, not a
    // per-sample scan), so a future regression that breaks the invariant panics
    // loudly here instead of minting a silently-malformed image.
    FilmRgbImage::from_linear(
        LinearImage::new(density.width, density.height, rgb, density.ir)
            .expect("the curve preserves the validated buffer-length invariants"),
    )
}

/// Reject a film base that would make the density conversion ill-defined: each
/// per-channel value is a transmission in `(0, 1]`. Non-positive / non-finite
/// values would divide into inf/NaN; values above `1.0` are impossible for a
/// `[0, 1]`-normalized scan (a typo like `--film-base 90` for `0.90`) and would
/// silently render every real sample above white.
pub(crate) fn check_base(base: &FilmBase) -> Result<()> {
    for (name, v) in [("r", base.r), ("g", base.g), ("b", base.b)] {
        if !v.is_finite() || v <= 0.0 || v > 1.0 {
            return Err(NcError::Other(format!(
                "film base {name} channel must be a transmission in (0, 1] (got {v}); \
                 measure a valid Dmin or pass an explicit --film-base"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::reconstruct as reconstruct_config;
    use crate::types::{AnchorPlacement, ExponentialParams, Reconstruction};

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    /// A 1×1 RGB image (optionally with a 1-sample IR plane) for pixel-math tests.
    fn pixel(rgb: [f32; 3], ir: Option<f32>) -> LinearImage {
        LinearImage::new(1, 1, rgb.to_vec(), ir.map(|v| vec![v])).unwrap()
    }

    /// The exponential curve carrying `gamma`, with mid-grey `offset` above the base.
    fn exponential(gamma: f32, offset: f32) -> DensityCurve {
        DensityCurve::Exponential(ExponentialParams {
            gamma,
            anchor: AnchorPlacement::MidAtBaseOffset(offset),
        })
    }

    /// The result of a full density-path reconstruction.
    #[derive(Debug)]
    struct Converted {
        out: LinearImage,
        curve_anchor: Option<f32>,
        balance_range: Option<[f32; 2]>,
    }

    /// Run the full density path through the public entry point, `reconstruct`
    /// (stages 1–3).
    fn run(
        img: &LinearImage,
        base: &FilmBase,
        density: DensityParams,
        curve: DensityCurve,
    ) -> Result<Converted> {
        let config = Reconstruction { density, curve };
        let (film, rep) = reconstruct_config(img, base, &config)?;
        Ok(Converted {
            out: film.into_linear(),
            curve_anchor: rep.curve_anchor,
            balance_range: rep.balance_range,
        })
    }

    /// The anchored exponential curve on a prepared density buffer (stage 3) — the
    /// same composition `reconstruct`'s exponential arm performs.
    fn render(density: DensityImage, gamma: f32, anchor: f32) -> LinearImage {
        apply_curve(density, move |d| 10f32.powf(gamma * (d - anchor))).into_linear()
    }

    // --- stage 1–2: to_density -------------------------------------------------

    /// `DensityParams` with an **identity** per-channel gain, for tests about the
    /// density transform itself rather than about the shipped default.
    ///
    /// The default gain is `[1, 0.84, 0.73]` — a scanner calibration, not part of the
    /// `D = −log10(scan / base)` definition — so a test asserting that definition, or
    /// asserting that equal base fractions give equal densities, has to state the
    /// identity or it is asserting the calibration instead.
    fn identity_gain() -> DensityParams {
        DensityParams {
            scale: [1.0, 1.0, 1.0],
            ..DensityParams::default()
        }
    }

    #[test]
    fn to_density_computes_neg_log10_ratio() {
        // base = 1 makes D = -log10(scan): 0.1 → 1, 0.01 → 2, 1.0 → 0.
        let img = pixel([0.1, 0.01, 1.0], None);
        let base = FilmBase::from([1.0, 1.0, 1.0]);
        let d = to_density(&img, &base, &identity_gain());
        assert!(approx(d.density[0], 1.0, 1e-5));
        assert!(approx(d.density[1], 2.0, 1e-5));
        assert!(approx(d.density[2], 0.0, 1e-5));
    }

    #[test]
    fn to_density_is_relative_to_per_channel_base() {
        // A neutral patch = the same fraction of each channel's base → equal density
        // across channels (this is the orange-mask removal). base is deliberately
        // orange (r>g>b); scan is 1/2 of base per channel.
        let base = FilmBase::from([0.5, 0.25, 0.15]);
        let img = pixel([0.25, 0.125, 0.075], None);
        let d = to_density(&img, &base, &identity_gain());
        let expected = -(0.5f32).log10(); // ≈ 0.30103
        for c in 0..3 {
            assert!(approx(d.density[c], expected, 1e-5), "channel {c}");
        }
    }

    #[test]
    fn to_density_applies_scale_then_offset() {
        // D = -log10(0.1/1) = 1; D' = scale·1 + offset.
        let img = pixel([0.1, 0.1, 0.1], None);
        let base = FilmBase::from([1.0, 1.0, 1.0]);
        let params = DensityParams {
            scale: [2.0, 1.0, 0.5],
            offset: [0.5, -0.25, 0.0],
            ..DensityParams::default()
        };
        let d = to_density(&img, &base, &params);
        assert!(approx(d.density[0], 2.5, 1e-5));
        assert!(approx(d.density[1], 0.75, 1e-5));
        assert!(approx(d.density[2], 0.5, 1e-5));
    }

    #[test]
    fn to_density_epsilon_clamp_keeps_zero_and_negative_finite() {
        // Zero / negative transmission (dead or noisy sample) must not become
        // -inf / NaN — the epsilon floor yields a high but finite density.
        let img = pixel([0.0, -5.0, f32::MIN_POSITIVE], None);
        let base = FilmBase::from([1.0, 1.0, 1.0]);
        let d = to_density(&img, &base, &identity_gain());
        for c in 0..3 {
            assert!(d.density[c].is_finite(), "channel {c} not finite");
        }
        // scan==0 and scan<0 both floor to the same SCAN_EPSILON-derived density.
        let expected = -(SCAN_EPSILON).log10();
        assert!(approx(d.density[0], expected, 1e-4));
        assert!(approx(d.density[1], expected, 1e-4));
    }

    #[test]
    fn to_density_carries_ir_untouched() {
        let img = pixel([0.2, 0.2, 0.2], Some(0.42));
        let base = FilmBase::from([1.0, 1.0, 1.0]);
        let d = to_density(&img, &base, &DensityParams::default());
        assert_eq!(d.ir.as_deref(), Some(&[0.42_f32][..]));
    }

    // --- stage 3–4: curve + print render ----------------------------------------

    #[test]
    fn render_maps_density_through_ten_to_the_power() {
        // Neutral print params, gamma 1: lin = 10^D'. D'=[1,0,2] → [10,1,100].
        let d = DensityImage {
            width: 1,
            height: 1,
            density: vec![1.0, 0.0, 2.0],
            ir: None,
        };
        let out = render(d, 1.0, 0.0);
        assert!(approx(out.rgb[0], 10.0, 1e-3));
        assert!(approx(out.rgb[1], 1.0, 1e-5));
        assert!(approx(out.rgb[2], 100.0, 1e-2));
    }

    #[test]
    fn render_gamma_scales_the_density_exponent() {
        // lin = 10^(gamma·D'); D'=1, gamma=0.5 → 10^0.5 ≈ 3.1623.
        let d = DensityImage {
            width: 1,
            height: 1,
            density: vec![1.0, 1.0, 1.0],
            ir: None,
        };
        let out = render(d, 0.5, 0.0);
        for c in 0..3 {
            assert!(approx(out.rgb[c], 10f32.powf(0.5), 1e-3), "channel {c}");
        }
    }

    #[test]
    fn render_carries_ir_untouched() {
        let d = DensityImage {
            width: 1,
            height: 1,
            density: vec![0.3, 0.3, 0.3],
            ir: Some(vec![0.7]),
        };
        let out = render(d, 1.0, 0.0);
        assert_eq!(out.ir.as_deref(), Some(&[0.7_f32][..]));
    }

    // --- reconstruction: composition + polarity ---------------------------------

    // Wiring test: confirms the full path = `to_density` then the anchored
    // exponential curve, with the right gamma threaded through (catches a
    // dropped/wrong gamma or a swapped stage).
    #[test]
    fn full_path_equals_render_of_to_density() {
        let img = pixel([0.3, 0.15, 0.08], Some(0.5));
        let base = FilmBase::from([0.6, 0.3, 0.18]);
        let density = DensityParams {
            scale: [1.1, 1.0, 0.9],
            offset: [0.05, 0.0, -0.05],
            ..DensityParams::default()
        };
        let gamma = 1.4;
        let curve = exponential(gamma, 0.5);
        let via_config = run(&img, &base, density.clone(), curve).unwrap();
        let dimg = to_density(&img, &base, &density);
        let anchor = curve.anchor().unwrap().anchor(gamma);
        let via_parts = render(dimg, gamma, anchor);
        assert_eq!(via_config.out.rgb, via_parts.rgb);
        assert_eq!(via_config.out.ir, via_parts.ir);
    }

    #[test]
    fn convert_is_positive_polarity_denser_is_brighter() {
        // Two pixels sharing a base: pixel 0 is thinner (near base → scene shadow),
        // pixel 1 is denser (lower transmission → scene highlight). A correct
        // positive renders the denser negative *brighter*. This is the guard that
        // pins the sign fix — a `10^(-γD')` regression would flip it.
        let base = FilmBase::from([0.6, 0.6, 0.6]);
        let img = LinearImage::new(2, 1, vec![0.55, 0.55, 0.55, 0.05, 0.05, 0.05], None).unwrap();
        let out = run(
            &img,
            &base,
            DensityParams::default(),
            DensityCurve::default(),
        )
        .unwrap()
        .out;
        for c in 0..3 {
            assert!(
                out.rgb[3 + c] > out.rgb[c],
                "denser pixel should be brighter (channel {c}): \
                 thin={} dense={}",
                out.rgb[c],
                out.rgb[3 + c]
            );
        }
    }

    #[test]
    fn convert_neutral_patch_stays_neutral() {
        // **What "neutral in" means is the whole content of this test, and the default
        // gain changed it.** Equal fractions of each base channel are neutral only if a
        // neutral *scene* produces equal densities — which it does not: each layer has
        // its own slope, so a real neutral exposes them apart. Both halves are asserted
        // because a regression in either is a colour bug that no other test sees.
        let base = FilmBase::from([0.5, 0.25, 0.15]);
        let neutral_out = |img, params| {
            run(&img, &base, params, DensityCurve::default())
                .unwrap()
                .out
        };

        // (a) Under the identity gain, equal base fractions still reconstruct neutral —
        // the structural orange-mask removal in `to_density`, unchanged.
        let out = neutral_out(pixel([0.2, 0.1, 0.06], None), identity_gain()); // 0.4 × base
        assert!(approx(out.rgb[0], out.rgb[1], 1e-4));
        assert!(approx(out.rgb[1], out.rgb[2], 1e-4));

        // (b) Under the shipped gain, a patch carrying the **measured** channel slope
        // ratios reconstructs *closer to* neutral. `algo::curve_probe::sigmoid_scale`
        // measures those ratios at green 1.115, blue 1.183 against red — so build the
        // patch from the ratios and assert the render improves it.
        //
        // **The patch is deliberately not rebuilt from the shipping gain, and the claim
        // is weaker since `pipeline_version` 5.** Two calibrations of this gain exist and
        // they disagree: the tone-scale slope over 21 frames nulls at `[1, 0.897, 0.845]`
        // (v4 shipped `[1, 0.90, 0.86]` from it and cancelled this patch ~4x), while the
        // 31 hand-marked neutral patches over five rolls null at `[1, 0.837, 0.733]`
        // (v5 ships `[1, 0.84, 0.73]`). On *this* patch — built from the slope corpus —
        // v5 leaves 0.1286 spread against 0.1791 uncorrected, a 1.39x improvement rather
        // than v4's 4x, because it corrects past what the slope measurement asked for.
        //
        // Rebuilding the patch from the neutral-patch ratios would make this assert that
        // the default nulls the data it was derived from, which is circular and would
        // delete the disagreement. Keeping it records that the two measurements do not
        // agree — the residual `io/scanner-density-calibration` owns — and still catches
        // a gain that stops correcting the measured slope at all.
        let (d_r, r_g, r_b) = (0.4f32, 1.115f32, 1.183f32);
        let transmission = |d: f32, b: f32| b * 10f32.powf(-d);
        let img = pixel(
            [
                transmission(d_r, 0.5),
                transmission(d_r * r_g, 0.25),
                transmission(d_r * r_b, 0.15),
            ],
            None,
        );
        // Asserted as an improvement *ratio* rather than an absolute tolerance: the
        // sigmoid's toe and shoulder are non-linear, so they amplify whatever density
        // residual survives by a local slope that is not the nominal contrast. The claim
        // the default makes is comparative — this patch is what it is calibrated for —
        // so compare it against the same patch under the identity gain.
        let spread = |out: &crate::types::LinearImage| {
            let m = (out.rgb[0] + out.rgb[1] + out.rgb[2]) / 3.0;
            (0..3)
                .map(|c| (out.rgb[c] - m).abs() / m)
                .fold(0.0f32, f32::max)
        };
        let corrected = spread(&neutral_out(img.clone(), DensityParams::default()));
        let uncorrected = spread(&neutral_out(img, identity_gain()));
        assert!(
            corrected < uncorrected / 1.25,
            "the default gain left {corrected:.4} spread on a scene-neutral patch built from \
             the measured slope ratios, against {uncorrected:.4} uncorrected — it must still \
             improve that patch, whatever else it is calibrated against"
        );
        assert!(
            corrected < 0.20,
            "residual spread {corrected:.4} is larger than either calibration predicts"
        );
    }

    #[test]
    fn to_density_propagates_non_finite_scan() {
        // NaN / +inf transmission must NOT be laundered to a finite density by the
        // epsilon floor — they propagate as NaN so io::encode's non-finite counter
        // surfaces corrupt input. A finite channel alongside them is unaffected.
        let img = pixel([f32::NAN, f32::INFINITY, 0.2], None);
        let base = FilmBase::from([1.0, 1.0, 1.0]);
        let d = to_density(&img, &base, &DensityParams::default());
        assert!(d.density[0].is_nan(), "NaN scan → NaN density");
        assert!(d.density[1].is_nan(), "+inf scan → NaN density");
        assert!(d.density[2].is_finite(), "finite scan stays finite");
    }

    #[test]
    fn convert_default_output_is_finite_no_blowup() {
        // "No channel blow-outs": a normal pixel under default params yields
        // finite, bounded output (not NaN/inf).
        let base = FilmBase::from([0.55, 0.27, 0.16]);
        let img = pixel([0.39, 0.19, 0.09], None);
        let out = run(
            &img,
            &base,
            DensityParams::default(),
            DensityCurve::default(),
        )
        .unwrap()
        .out;
        for c in 0..3 {
            assert!(out.rgb[c].is_finite(), "channel {c} not finite");
            assert!(out.rgb[c] < 1000.0, "channel {c} blew up: {}", out.rgb[c]);
        }
    }

    #[test]
    fn convert_rejects_non_positive_or_non_finite_base() {
        let img = pixel([0.2, 0.2, 0.2], None);
        for bad in [
            [1.0, 0.0, 1.0],      // zero channel → division by zero
            [1.0, -0.5, 1.0],     // negative transmission
            [f32::NAN, 1.0, 1.0], // non-finite
            [1.0, f32::INFINITY, 1.0],
            [1.0, 90.0, 1.0], // transmission > 1 (e.g. "90" typo for "0.90")
        ] {
            let err = run(
                &img,
                &FilmBase::from(bad),
                DensityParams::default(),
                DensityCurve::default(),
            )
            .unwrap_err();
            assert_eq!(err.exit_code(), 1, "base {bad:?} should fail loudly");
        }
        // A valid base still converts.
        assert!(
            run(
                &img,
                &FilmBase::from([0.5, 0.5, 0.5]),
                DensityParams::default(),
                DensityCurve::default(),
            )
            .is_ok()
        );
    }

    #[test]
    fn anchored_exponent_survives_extreme_gamma_and_anchor() {
        // Regression (PR review): the anchor used to be a separate 10^(−γ·A)
        // gain, so γ·D' alone could overflow f32 before the gain cancelled it
        // (γ = 5, D' = 8 ⇒ 10^40 = inf ⇒ white rendered inf/NaN). With the
        // anchored exponent, D' = A maps to exactly 1.0 regardless of scale.
        let gamma = 5.0f32;
        let anchor = 8.0f32;
        let dimg = DensityImage {
            width: 1,
            height: 1,
            density: vec![anchor, anchor, anchor],
            ir: None,
        };
        let out = render(dimg, gamma, anchor);
        for v in &out.rgb {
            assert!(v.is_finite(), "overflowed: {v}");
            assert!(approx(*v, 1.0, 1e-5), "scene white should be 1.0, got {v}");
        }
    }

    #[test]
    fn convert_preserves_ir_plane() {
        let base = FilmBase::from([1.0, 1.0, 1.0]);
        let img = pixel([0.2, 0.2, 0.2], Some(0.33));
        let out = run(
            &img,
            &base,
            DensityParams::default(),
            DensityCurve::default(),
        )
        .unwrap()
        .out;
        assert_eq!(out.ir.as_deref(), Some(&[0.33_f32][..]));
    }

    // --- regional (shadow/highlight) balance ------------------------------------

    /// A `DensityImage` straight from interleaved corrected densities.
    fn density_image(width: u32, height: u32, density: Vec<f32>) -> DensityImage {
        DensityImage {
            width,
            height,
            density,
            ir: None,
        }
    }

    #[test]
    fn consults_balance_range_matches_regional_balance_observable_behaviour() {
        // `cli::validate_output_preset` rejects a frame-local `Auto` range under
        // `film-master` only when it is genuinely measured, using this predicate — so
        // the predicate must agree with `regional_balance`'s own short-circuits, or
        // the master would either reject an inert default or accept a real per-frame
        // measurement. `regional_balance` reports `Some(range)` exactly when it
        // consulted one, which makes the agreement directly observable.
        let cases = [
            ("neutral default", [0.0; 3], [0.0; 3]),
            ("equal non-neutral", [0.05, 0.0, -0.02], [0.05, 0.0, -0.02]),
            ("negative zero vs zero", [-0.0; 3], [0.0; 3]),
            ("shadow only", [0.05, 0.0, -0.02], [0.0; 3]),
            ("highlight only", [0.0; 3], [-0.05, 0.01, 0.0]),
            ("both, unequal", [0.05, 0.0, -0.02], [-0.05, 0.01, 0.0]),
            ("differ in one channel", [0.05, 0.0, 0.0], [0.05, 0.0, 0.01]),
        ];
        for (name, shadow_balance, highlight_balance) in cases {
            let params = DensityParams {
                shadow_balance,
                highlight_balance,
                // Explicit, so the "consulted" case cannot fail on an unmeasurable
                // frame and muddy the comparison.
                balance_range: BalanceRange::Explicit([0.5, 2.5]),
                ..DensityParams::default()
            };
            let mut img = density_image(2, 1, vec![0.2, 0.0, 0.0, 2.8, 3.0, 3.0]);
            let consulted = regional_balance(&mut img, &params).unwrap().is_some();
            assert_eq!(
                consulted,
                consults_balance_range(&params),
                "{name}: predicate disagrees with regional_balance"
            );
        }
    }

    #[test]
    fn regional_balance_neutral_default_is_bit_exact_noop() {
        // Zero balances must not touch the buffer at all — not even `+0.0`
        // (which would flip a `-0.0`) or NaN arithmetic — and report no range.
        let src = vec![0.7f32, -0.0, f32::NAN, 0.0, 2.0, -1.1];
        let mut img = density_image(2, 1, src.clone());
        let resolved = regional_balance(&mut img, &DensityParams::default()).unwrap();
        assert_eq!(resolved, None);
        for (a, b) in img.density.iter().zip(&src) {
            assert_eq!(a.to_bits(), b.to_bits(), "buffer must be bit-identical");
        }
    }

    #[test]
    fn regional_balance_neutralizes_a_synthetic_crossover() {
        // Opposite casts injected at low/high density: shadows too red (+0.2 red
        // density), highlights too cyan (−0.2 red density). Matching balance
        // params neutralize both — the definition of the control. The explicit
        // range [0.5, 2.5] keeps both pixels in the saturated weight zones
        // (tones ≈ 0.07 and ≈ 2.93), so the correction is exact.
        let mut img = density_image(2, 1, vec![0.2, 0.0, 0.0, 2.8, 3.0, 3.0]);
        let params = DensityParams {
            shadow_balance: [-0.2, 0.0, 0.0],
            highlight_balance: [0.2, 0.0, 0.0],
            balance_range: BalanceRange::Explicit([0.5, 2.5]),
            ..DensityParams::default()
        };
        let resolved = regional_balance(&mut img, &params).unwrap();
        assert_eq!(resolved, Some([0.5, 2.5]));
        for c in 0..3 {
            assert!(approx(img.density[c], 0.0, 1e-6), "shadow chan {c}");
            assert!(approx(img.density[3 + c], 3.0, 1e-6), "highlight chan {c}");
        }
    }

    #[test]
    fn smoothstep_is_clamped_smooth_and_monotonic() {
        // Endpoints and saturation outside the range.
        assert_eq!(smoothstep(-1.0), 0.0);
        assert_eq!(smoothstep(0.0), 0.0);
        assert_eq!(smoothstep(1.0), 1.0);
        assert_eq!(smoothstep(2.0), 1.0);
        // Midpoint is the 50/50 blend; monotonic non-decreasing across the range.
        assert!(approx(smoothstep(0.5), 0.5, 1e-6));
        let mut prev = 0.0f32;
        for i in 0..=100 {
            let w = smoothstep(i as f32 / 100.0);
            assert!(w >= prev, "not monotonic at {i}");
            prev = w;
        }
        // Smooth at the ends: near-zero slope (first-order flat).
        assert!(smoothstep(0.01) < 0.001);
        assert!(smoothstep(0.99) > 0.999);
    }

    #[test]
    fn regional_balance_uses_one_scalar_tone_for_all_channels() {
        // A crossover pixel whose channels straddle the range: per-channel
        // weighting would give channel 0 the full shadow correction and channel
        // 1 the full highlight one — exactly the misfire the scalar tone
        // prevents. Tone = mean(0, 3, 1.5) = 1.5 = range midpoint ⇒ every
        // channel gets the same 0.5/0.5 blend.
        let mut img = density_image(1, 1, vec![0.0, 3.0, 1.5]);
        let params = DensityParams {
            shadow_balance: [1.0, 1.0, 1.0],
            highlight_balance: [0.0, 0.0, 0.0],
            balance_range: BalanceRange::Explicit([0.0, 3.0]),
            ..DensityParams::default()
        };
        regional_balance(&mut img, &params).unwrap();
        assert!(approx(img.density[0], 0.5, 1e-6));
        assert!(approx(img.density[1], 3.5, 1e-6));
        assert!(approx(img.density[2], 2.0, 1e-6));
    }

    #[test]
    fn regional_balance_equal_balances_reduce_to_a_uniform_offset() {
        // w_lo + w_hi = 1 (complementary ramps), so equal shadow and highlight
        // balances act as one tone-independent per-channel offset.
        let offs = [0.1f32, -0.05, 0.02];
        let src = vec![0.0f32, 0.5, 1.0, 2.0, 2.5, 3.0];
        let mut img = density_image(2, 1, src.clone());
        let params = DensityParams {
            shadow_balance: offs,
            highlight_balance: offs,
            balance_range: BalanceRange::Explicit([0.0, 3.0]),
            ..DensityParams::default()
        };
        regional_balance(&mut img, &params).unwrap();
        for (i, (&got, &was)) in img.density.iter().zip(&src).enumerate() {
            assert!(approx(got, was + offs[i % 3], 1e-6), "sample {i}");
        }
    }

    #[test]
    fn regional_balance_equal_balances_need_no_range_on_a_uniform_frame() {
        // Equal (non-neutral) balances reduce to a per-channel offset that never
        // uses the range (w_lo + w_hi = 1), so `Auto` must NOT try to measure a
        // range: on a uniform frame the measurement is degenerate and would error.
        // The equal-balances short-circuit applies the constant offset and reports
        // no range (design-spec §7.2). Regression guard for the Codex P2.
        let offs = [0.1f32, 0.0, -0.1];
        let src = vec![1.5f32; 12]; // uniform ⇒ measure_balance_range → None
        let mut img = density_image(4, 1, src.clone());
        let params = DensityParams {
            shadow_balance: offs,
            highlight_balance: offs,
            balance_range: BalanceRange::Auto, // must not be consulted
            ..DensityParams::default()
        };
        // Succeeds (no "cannot measure a density range" error) and reports None.
        let resolved = regional_balance(&mut img, &params).unwrap();
        assert_eq!(resolved, None, "equal balances consult no range");
        // Output == neutral frame + the constant per-channel offset.
        for (i, (&got, &was)) in img.density.iter().zip(&src).enumerate() {
            assert!(approx(got, was + offs[i % 3], 1e-6), "sample {i}");
        }
    }

    #[test]
    fn measure_balance_range_takes_nearest_rank_percentiles() {
        // 1000 pixels with distinct tones 0..=999: lo = round(999·0.005) = 5,
        // hi = round(999·0.995) = 994 — pins both nearest-rank indices.
        let density: Vec<f32> = (0..1000).flat_map(|t| [t as f32; 3]).collect();
        assert_eq!(measure_balance_range(&density), Some([5.0, 994.0]));
    }

    #[test]
    fn measure_balance_range_rejects_degenerate_distributions() {
        // Uniform (lo == hi), all-non-finite, and empty all yield None.
        assert_eq!(measure_balance_range(&[1.0; 30]), None);
        assert_eq!(measure_balance_range(&[f32::NAN; 30]), None);
        assert_eq!(measure_balance_range(&[]), None);
        // Individually-finite tones whose span overflows f32 → None (a flat,
        // silently-wrong ramp otherwise). Two pixels at ∓f32::MAX tones.
        let m = f32::MAX;
        assert_eq!(measure_balance_range(&[-m, -m, -m, m, m, m]), None);
        // Non-finite tones are excluded, not ranked: the finite pixels alone
        // define the range.
        let mut density: Vec<f32> = (0..200).flat_map(|t| [t as f32; 3]).collect();
        density.extend([f32::NAN; 3]);
        let [lo, hi] = measure_balance_range(&density).unwrap();
        assert!(lo.is_finite() && hi.is_finite() && lo < hi);
    }

    #[test]
    fn regional_balance_auto_fails_loudly_on_a_uniform_frame() {
        // A requested balance with no measurable range must error (a silently
        // skipped correction is a quietly wrong image), naming the recovery flag.
        let mut img = density_image(2, 1, vec![1.0; 6]);
        let params = DensityParams {
            shadow_balance: [0.1, 0.0, 0.0],
            ..DensityParams::default()
        };
        let err = regional_balance(&mut img, &params).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert!(err.to_string().contains("--balance-range"));
        // An explicit range short-circuits the measuring pass and succeeds on
        // the same frame (the documented roll-reuse escape hatch).
        let params = DensityParams {
            balance_range: BalanceRange::Explicit([0.0, 2.0]),
            ..params
        };
        assert_eq!(
            regional_balance(&mut img, &params).unwrap(),
            Some([0.0, 2.0])
        );
    }

    #[test]
    fn regional_balance_keeps_non_finite_channels_confined() {
        // One NaN channel: the tone comes from the finite channels, which still
        // receive their correction; the NaN channel stays NaN (the encoder's
        // non-finite counter must still see it). An all-NaN pixel is untouched.
        let mut img = density_image(2, 1, vec![f32::NAN, 1.0, 2.0, f32::NAN, f32::NAN, f32::NAN]);
        let params = DensityParams {
            shadow_balance: [0.5, 0.5, 0.5],
            highlight_balance: [0.5, 0.5, 0.5], // uniform +0.5 (tone-independent)
            balance_range: BalanceRange::Explicit([0.0, 3.0]),
            ..DensityParams::default()
        };
        regional_balance(&mut img, &params).unwrap();
        assert!(img.density[0].is_nan());
        assert!(approx(img.density[1], 1.5, 1e-6));
        assert!(approx(img.density[2], 2.5, 1e-6));
        for c in 3..6 {
            assert!(img.density[c].is_nan(), "all-NaN pixel must stay NaN");
        }
    }

    #[test]
    fn pixel_tone_rejects_an_overflowing_mean() {
        // Finite channels near f32::MAX can overflow the sum to +inf. An
        // infinite tone must not leak out: as a measured anchor it would make
        // `hi = inf` (a flat, silently-wrong ramp), and in the apply pass it
        // would become NaN weights poisoning the pixel's finite channels.
        let huge = f32::MAX;
        assert_eq!(pixel_tone(&[huge, huge, huge]), None);
        // A normal pixel and a partially-finite pixel still produce a tone.
        assert_eq!(pixel_tone(&[1.0, 2.0, 3.0]), Some(2.0));
        assert_eq!(pixel_tone(&[f32::NAN, 1.0, 3.0]), Some(2.0));
        assert_eq!(pixel_tone(&[f32::NAN; 3]), None);

        // End to end: the overflow pixel neither anchors the measured range
        // nor receives a correction; the well-behaved pixels still do. Uses
        // *unequal* shadow/highlight balances so the `Auto` range-measuring path
        // actually runs (equal balances short-circuit to a constant offset before
        // any measurement — see `regional_balance`). Finite tones are 0.0 and 2.0,
        // so the measured range is [0.0, 2.0]: the tone-0 pixel sits at w_lo = 1
        // (gets +0.1) and the tone-2 pixel at w_hi = 1 (gets +0.3).
        let mut img = density_image(3, 1, vec![huge, huge, huge, 0.0, 0.0, 0.0, 2.0, 2.0, 2.0]);
        let params = DensityParams {
            shadow_balance: [0.1, 0.1, 0.1],
            highlight_balance: [0.3, 0.3, 0.3],
            ..DensityParams::default()
        };
        let resolved = regional_balance(&mut img, &params).unwrap().unwrap();
        assert!(resolved[1].is_finite(), "hi anchor must not be inf");
        for c in 0..3 {
            assert_eq!(img.density[c], huge, "overflow pixel untouched");
            assert!(approx(img.density[3 + c], 0.1, 1e-6));
            assert!(approx(img.density[6 + c], 2.3, 1e-6));
        }
    }

    #[test]
    fn reconstruct_surfaces_the_balance_range() {
        let base = FilmBase::from([0.6, 0.6, 0.6]);
        // Two-tone image so an Auto range is measurable.
        let img = LinearImage::new(2, 1, vec![0.5, 0.5, 0.5, 0.05, 0.05, 0.05], None).unwrap();

        // Neutral balances → no range reported (and no measuring pass).
        let rep = run(
            &img,
            &base,
            DensityParams::default(),
            DensityCurve::default(),
        )
        .unwrap();
        assert_eq!(rep.balance_range, None);

        // Auto → the measured [lo, hi], finite and ordered.
        let rep = run(
            &img,
            &base,
            DensityParams {
                shadow_balance: [0.05, 0.0, 0.0],
                ..DensityParams::default()
            },
            DensityCurve::default(),
        )
        .unwrap();
        let [lo, hi] = rep.balance_range.expect("range reported");
        assert!(lo.is_finite() && hi.is_finite() && lo < hi);

        // Explicit → echoed back exactly.
        let rep = run(
            &img,
            &base,
            DensityParams {
                shadow_balance: [0.05, 0.0, 0.0],
                balance_range: BalanceRange::Explicit([0.25, 1.75]),
                ..DensityParams::default()
            },
            DensityCurve::default(),
        )
        .unwrap();
        assert_eq!(rep.balance_range, Some([0.25, 1.75]));
    }

    #[test]
    fn regional_balance_is_deterministic_and_preserves_ir() {
        let base = FilmBase::from([0.6, 0.3, 0.18]);
        let img = LinearImage::new(
            3,
            1,
            vec![0.5, 0.25, 0.15, 0.3, 0.15, 0.09, 0.1, 0.05, 0.03],
            Some(vec![0.1, 0.2, 0.3]),
        )
        .unwrap();
        let params = DensityParams {
            shadow_balance: [0.1, 0.0, -0.05],
            highlight_balance: [-0.1, 0.02, 0.0],
            ..DensityParams::default()
        };
        let a = run(&img, &base, params.clone(), DensityCurve::default())
            .unwrap()
            .out;
        let b = run(&img, &base, params, DensityCurve::default())
            .unwrap()
            .out;
        assert_eq!(a.rgb, b.rgb);
        assert_eq!(a.ir.as_deref(), Some(&[0.1f32, 0.2, 0.3][..]));
    }

    // --- the anchor ------------------------------------------------------------

    #[test]
    fn the_anchor_density_maps_to_display_white() {
        // The pixel at `D' = A` renders to exactly 1.0, and the base (`D' = 0`) to
        // `10^(−γ·A) < 1` (near black).
        let anchor = 1.5f32;
        let gamma = 2.0f32;
        let dimg = DensityImage {
            width: 2,
            height: 1,
            density: vec![anchor, anchor, anchor, 0.0, 0.0, 0.0],
            ir: None,
        };
        let out = render(dimg, gamma, anchor);
        for c in 0..3 {
            assert!(approx(out.rgb[c], 1.0, 1e-5), "anchor → 1.0 (chan {c})");
            assert!(approx(out.rgb[3 + c], 10f32.powf(-gamma * anchor), 1e-6));
            assert!(out.rgb[3 + c] < 1.0, "base below white (chan {c})");
        }
    }

    #[test]
    fn reconstruct_surfaces_the_resolved_anchor() {
        let base = FilmBase::from([0.6, 0.6, 0.6]);
        let img = pixel([0.2, 0.2, 0.2], None);

        // The exponential reports the anchor its placement derived…
        let curve = exponential(1.0, 0.5);
        let rep = run(&img, &base, DensityParams::default(), curve).unwrap();
        assert_eq!(rep.curve_anchor, Some(curve.anchor().unwrap().anchor(1.0)));

        // …and the characteristic curve, which places none, reports none.
        let characteristic =
            DensityCurve::Characteristic(crate::types::CharacteristicParams::default());
        let rep = run(&img, &base, DensityParams::default(), characteristic).unwrap();
        assert_eq!(rep.curve_anchor, None);
    }
}
