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
//! tone in a deterministic two-pass within stage 2 (it cannot anchor on the
//! `Auto` `Dmax`, which is measured *after* stage 2 — that would be circular);
//! `Explicit` short-circuits the measuring pass for roll reuse. The neutral
//! `[0,0,0]` defaults skip the pass entirely — bit-exact with the unbalanced
//! output. `Dmax` (`Auto`) is then measured from the *post-balance* densities,
//! keeping the display-white anchor consistent with what is rendered.
//!
//! **Display-white anchor (`Dmax`) — owned by the curve stage.** The
//! exponential curve renders density *relative to* the scene-white density
//! `Dmax`: scene white (`D' = Dmax`) maps to `1.0` and the base (`D' = 0`) to
//! `10^(−γ·Dmax) ≈ 0`, so the default u16 encode fills the display range
//! instead of leaving every real sample above `1.0`. `10^(γ·(D'−Dmax))`
//! factors into `10^(γ·D')` times a constant gain `10^(−γ·Dmax)`, so the
//! anchor composes with `print_exposure` as one multiplicative scalar. The
//! anchor source is [`DmaxSource`], carried by the curve variant: `Fixed`
//! (default) uses the roll-fixed nominal [`NOMINAL_DMAX`]; `Explicit` fixes it
//! to a measured-reference / per-stock scalar; `Auto` (demoted, opt-in)
//! measures it per frame from the corrected-density distribution; `None`
//! (exponential only) disables it (gain `1.0`) and reproduces the unanchored
//! render bit-for-bit. Like the `Dmin` base, `Dmax` is a **roll-fixed**
//! calibration by default (a film + scanner property) — `Auto`'s per-frame
//! measurement is exposure normalization, not the faithful-conversion default
//! (see [`DmaxSource`] / the `dmax-reference` task). A `Dmax` measured once
//! from a fully-exposed reference frame is [`reference_dmax`]; it reduces to a
//! plain scalar, so a reference-derived anchor and an equal explicit `--d-max`
//! render **identical** color (no per-channel term).
//!
//! **Polarity.** With `D = -log10(scan/base)` the density is `≥ 0` and *grows*
//! with the film's optical density: the unexposed base (scene black) sits at
//! `D = 0`, and a dense negative area (a scene highlight) has a large `D`. A
//! true positive must get *brighter* as `D` grows, so stage 3 uses
//! `10^(+γ·D')`. This matches darktable `negadoctor`, whose print output
//! increases with film density (verified against its source: denser negative →
//! brighter print).
//!
//! Output is linear. With the default `Fixed` anchor scene white lands near
//! `1.0` (display-range-filling); with `--no-d-max` the base maps to `1.0` and
//! exposed detail sits above it (HDR / **scene-referred**), consistent with the
//! project's "don't clamp before encode" rule. Nothing is clamped here either
//! way — the encode stage counts and reports any out-of-range samples.

use rayon::prelude::*;

use crate::algo::{FilmRgbImage, ReconstructionReport};
use crate::types::{
    AnchorPlacement, BalanceRange, DensityCurve, DensityParams, DmaxInput, DmaxSource, FilmBase,
    LinearImage, NcError, Result,
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
/// resolve the curve's display-white anchor, then map `D′` through the selected
/// curve into the typed [`FilmRgbImage`]. Pure; the print controls are
/// deliberately **not** here — they run past the ACEScg boundary
/// (`pipeline::render_split`).
pub(super) fn reconstruct(
    image: &LinearImage,
    base: &FilmBase,
    params: &DensityParams,
    curve: &DensityCurve,
    dmax: DmaxInput,
) -> Result<(FilmRgbImage, ReconstructionReport)> {
    // `to_density` divides by the per-channel base, so a zero / negative /
    // non-finite base would yield a silently-black or non-finite image. The CLI
    // validates an *explicit* base, but an auto/region-estimated one is only
    // guarded here — the base's consumption point. Fail loudly instead.
    check_base(base)?;
    let mut density = to_density(image, base, params);

    // Regional (shadow/highlight) balance completes stage 2 *before* the curve
    // resolves an Auto `Dmax`, so the anchor is measured from the post-balance
    // densities (see the module doc for why the Auto anchor cannot precede the
    // balance).
    let balance_range = regional_balance(&mut density, params)?;

    let mut characteristic_out_of_table = None;
    let (film, dmax, curve_anchor) = match curve {
        DensityCurve::Exponential(exp) => {
            // Resolve the anchor once, from the (post-balance) corrected
            // densities. It is applied **in the exponent** — `10^(γ·(D' −
            // Dmax))` — not as a separate `10^(−γ·Dmax)` gain: mathematically
            // equivalent, but the factored form overflows `f32` when `γ·D'`
            // alone exceeds the pow10 range even though the anchored exponent
            // is small (e.g. `γ = 5`, EPS-clamped `D' ≈ 8`), turning scene
            // white into `inf` instead of `1.0`. A `None` anchor is applied as
            // exactly `0.0`, so it reproduces the unanchored render bit-for-bit
            // (`d − 0.0 == d` for every `f32`).
            let dmax = resolve_dmax(&density, dmax);
            let gamma = exp.gamma;
            // `WhiteAtDmax` over a `None` reference resolves `A = 0.0`, reproducing the
            // unanchored render bit-for-bit exactly as `dmax.unwrap_or(0.0)` did before
            // this curve had a placement rule. The base-derived placements ignore the
            // reference, so a `None` there is not a missing input.
            let anchor = exp.anchor.anchor(dmax.unwrap_or(0.0), gamma);
            // Defense in depth. Two ways the exponent
            // goes non-finite, and **both** render `10^(−inf) = 0.0` for every sample —
            // an all-black frame that trips neither the clip nor the non-finite counter:
            // the placement's division by the slope can overflow the *anchor* (a
            // positive-but-tiny gamma), and a large-but-finite anchor can overflow the
            // *product* `gamma · anchor` (reachable at the shipped default gamma). Only
            // those two: `A = 0.0` is the legitimate unity placement on this curve, and a
            // finite product is honest arithmetic whatever it renders.
            // `validate` rejects both at the CLI boundary, naming the flag; a
            // programmatic caller reaches here first.
            if !anchor.is_finite() || !(gamma * anchor).is_finite() {
                return Err(NcError::Other(format!(
                    "the exponential anchor placement derived a non-usable anchor \
                     ({anchor:e}) from reference {} at gamma {gamma}: the curve's exponent \
                     `gamma · (density − anchor)` is not finite, so every sample would \
                     render as exactly 0.0. Use a photographic gamma and a smaller anchor \
                     placement",
                    dmax.unwrap_or(0.0)
                )));
            }
            let film = apply_curve(density, move |d| 10f32.powf(gamma * (d - anchor)));
            // Under `WhiteAtDmax` the reference *is* the anchor, so `curve_anchor`
            // carries the reference verbatim — including its `None` — which is what the
            // report emitted before placement existed. Any other rule derives an anchor
            // that differs from the reference, and the report must show the derived one.
            let curve_anchor = match exp.anchor {
                AnchorPlacement::WhiteAtDmax => dmax,
                _ => Some(anchor),
            };
            (film, dmax, curve_anchor)
        }
        DensityCurve::Characteristic(ch) => {
            // No reference and no anchor to resolve: the published curve carries both, so
            // the report's `dmax` / `curve_anchor` are `None` rather than a derived number
            // that nothing consulted.
            crate::algo::film_stock::check_tables(ch.stock)?;
            let (film, out_of_table) = crate::algo::film_stock::apply_curve(density, ch.stock)?;
            characteristic_out_of_table = Some(out_of_table);
            (film, None, None)
        }
    };
    Ok((
        film,
        ReconstructionReport {
            dmax,
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
/// tonal range. `hi` mirrors [`AUTO_DMAX_PERCENTILE`]'s robustness intent.
const BALANCE_LO_PERCENTILE: f32 = 0.005;
const BALANCE_HI_PERCENTILE: f32 = 0.995;

/// Cap on how many per-pixel tones the `Auto` range measurement examines — the
/// same bound (and rationale) as [`AUTO_DMAX_MAX_SAMPLES`]: percentiles over ~1M
/// samples are statistically indistinguishable from the full population, and the
/// cap keeps the measuring pass to a small transient buffer. The stride walks
/// whole RGB pixels (chunks of 3), so no channel-bias adjustment is needed.
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

/// Percentile of the corrected-density distribution taken as the `Auto` anchor.
/// High enough to sit at genuine scene white while ignoring the top fraction of a
/// percent (specular sparkle, dust, hot pixels) that would otherwise anchor white
/// too bright and leave the image dim. Mirrors the robustness intent of
/// `film_base`'s sampling percentile, applied to the density (not transmission)
/// distribution.
const AUTO_DMAX_PERCENTILE: f32 = 0.995;

/// Nominal roll-fixed display-white anchor density — the [`DmaxSource::Fixed`]
/// default. A scene-independent placement expressed **in corrected-density (`D′`)
/// units** (where the base is `0`), *not* a base transmission plus a range (mixing
/// transmission and density is a unit error). It is the last tier of the fixed
/// resolution ladder (measured reference → per-stock constant → this nominal): the
/// value used when no reference / per-stock `Dmax` has been calibrated, so the
/// default u16 encode fills the display range while keeping relative exposure
/// faithful (darker frames stay darker).
///
/// **`1.3`, a rounded nominal chosen against measurement (2026-08-08).** The seven
/// rolls measured in this repo span **0.90 to 1.74**, median ≈1.34: Harman Phoenix
/// 0.8976 at the bottom and Portra 400 1.7383 at the top, with Gold 200 1.2758,
/// Ektar 1.2933 and Portra 160 1.3816 clustered in between. `1.3` is that median
/// rounded to one decimal — deliberately **not** presented as calibrated, and
/// deliberately not restated to more precision than n=7 rolls supports.
///
/// The previous `2.0` sat above *every* one of those rolls, and because the
/// exponential curve renders `10^(γ·(D′ − Dmax))`, an anchor 0.7 too high darkens
/// the whole frame by that many decades: on the Ektar reference frame it rendered
/// 5.09x darker in linear terms than the roll's own measured anchor (encoded means
/// 0.104 vs 0.259). A default no real roll reaches is not a conservative default,
/// it is a wrong one. Measured in `docs/reports/render-defaults-v2.md`.
///
/// Phoenix's 0.8976 is counted in that spread on purpose: it is the **worst case,
/// showing what the floor of the population looks like**, not an outlier excluded
/// to flatter the number. Whether a calibrated constant should exclude such stocks
/// is `film-base/dmax-anchor-reliability`'s call — that task still owns the number,
/// and it is open precisely because the leader-measured anchor's *level* is
/// uncontrolled (two rolls of one stock 0.295 apart while their bases agree to
/// 0.0005). Measure a stock-specific value with `estimate --d-max-region` (see
/// [`reference_dmax`]) and pass it via `--d-max` whenever accuracy matters;
/// per-stock constants belong to `algo/film-stock-profiles`.
pub(crate) const NOMINAL_DMAX: f32 = 1.3;

/// Resolve the display-white anchor density for a corrected-density buffer.
/// `Fixed` returns the roll-fixed nominal [`NOMINAL_DMAX`] (scene-independent, so
/// the buffer is not consulted); `Explicit` returns the given roll-fixed value;
/// `Auto` (opt-in) measures a high percentile of the *finite* densities (scalar,
/// pooled across channels — a per-channel anchor would double as color correction,
/// which is the auto-WB modes' job, see [`white_balance::estimate_gains`](crate::pipeline::white_balance::estimate_gains)); `None` yields no
/// anchor. Deterministic: same buffer + params ⇒ same value.
pub(crate) fn resolve_dmax(density: &DensityImage, dmax: DmaxInput) -> Option<f32> {
    match dmax.source {
        DmaxSource::None => None,
        DmaxSource::Fixed => Some(NOMINAL_DMAX),
        DmaxSource::Explicit(d) => Some(d),
        DmaxSource::Auto => Some(auto_dmax(density, dmax.region)),
    }
}

/// Smallest gray density a measured reference is *expected* to reach for a
/// genuinely fully-exposed leader. A light-struck leader is the film's max-density
/// endpoint — typically `≈ 2–3` density (its transmission is a few percent of the
/// base or less). A measured reference much below this is more likely a mid-tone
/// frame than a leader, so it yields a too-low anchor that silently blows the roll
/// too bright. We do **not** hard-reject it (thin / unusual stock and short
/// development legitimately vary), but flag it as a loud, `--strict`-promotable
/// warning for the user's manual review. `1.0` is deliberately conservative — a
/// full density decade below the base, well under a real leader's `≈ 2–3` — so it
/// fires only on clearly-implausible regions and does not false-alarm on a thin
/// but real leader.
pub(crate) const MIN_PLAUSIBLE_REFERENCE_DMAX: f32 = 1.0;

/// A measured reference `Dmax`: the roll-fixed scalar anchor plus the per-channel
/// base-relative densities it was reduced from. The `scalar` is the value frozen
/// into a recipe / passed via `--d-max`; `per_channel` exists so the caller's
/// plausibility check can look at the *weakest* channel — a colored/wrong region
/// can average to a plausible gray density while one channel is essentially
/// unexposed base, which the scalar alone hides (see [`reference_dmax`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReferenceDmax {
    /// The gray-mean anchor (mean of `per_channel`) — the scalar `Dmax`.
    pub scalar: f32,
    /// Per-channel base-relative densities `[r, g, b]` (`D_c = -log10(t_c/base_c)`).
    pub per_channel: [f32; 3],
}

/// Measure the roll-fixed display-white anchor `Dmax` (a scalar density) from a
/// **fully-exposed reference frame** (the light-struck roll leader — near-opaque
/// in every channel, always present, the film's max-density endpoint). This is the
/// *plan-phase* measurement behind `estimate --d-max-region`; the resolved scalar
/// is frozen into a roll recipe as `calibration.dmax = {"explicit": <d>}`
/// and reused across the roll exactly like an explicit `Dmin` base — the reference
/// frame / region is recorded only as report provenance, never as a re-read
/// directive (that would break the deterministic-apply contract).
///
/// `reference` is the reference region's representative per-channel transmission
/// (a robust central measure — the median — over the region's interior, sampled by
/// the caller). Each channel is converted to **base-relative density**
/// `D_c = -log10(t_c / base_c)` (raw `D` per design-spec §4; this equals the
/// corrected density only under an *identity* `scale = 1` / `offset = 0`, which the
/// parametric curves' default scale is not — `cli::explicit_dmax_domain_warning`
/// warns when a measured value is reused across that mismatch), then the
/// three are averaged to one **scalar** (a gray/luma reduction). Keeping `Dmax`
/// scalar is deliberate: a per-channel anchor would apply three different gains in
/// `10^(γ·(D′−Dmax))`, i.e. a white balance, which is the print-render stage's
/// job, not the anchor's (density conversion ≠ print rendering).
///
/// **Domain caveats (the anchor is only in the curve's domain under the defaults).**
/// The curve subtracts `Dmax` from the *corrected* density `D′ = scale·D + offset`
/// (then regional balance), but this measurement is raw `D`. So a frozen `--d-max`
/// is in the curve's domain only when `scale = 1` / `offset = 0` **and** the
/// shadow/highlight balance is neutral. Non-default `scale`/`offset` shift the
/// whole domain (a uniform, foldable offset — `cli::run_convert` warns,
/// `--strict`-promotable, when an explicit `--d-max` is combined with non-default
/// `scale`/`offset`); a non-neutral regional balance is **spatial**
/// (tone-dependent) and cannot fold into any scalar anchor at all — re-measure the
/// reference under the same density params, or keep them at their defaults.
///
/// `base` is the resolved `Dmin` and must be finite-and-positive per channel (the
/// caller guarantees this via `film_base`'s guard). Fails loudly ([`NcError::Other`],
/// exit 1) — never launders a degenerate region into a silently-wrong anchor —
/// when, on **any** channel:
/// - the transmission is non-finite or at/below the `SCAN_EPSILON` floor (an
///   effectively-zero sample: dead sensor, clipped black, or the dark holder beside
///   the leader). Unlike [`to_density`]'s dead-pixel floor, a *reference* region is
///   a calibration input, so a floored channel is a hard error here — it must not
///   manufacture a huge density and freeze a black-rendering anchor (mirrors the
///   `Dmin` "dark holder → zero channel errors loudly" guard); or
/// - the base-relative density is not positive — the region out-transmits the film
///   base on that channel, so it is not a fully-exposed reference (a leader is
///   denser than the base in **every** channel, not just on the gray average — a
///   colored/wrong region can average positive while one channel out-transmits).
///
/// A measured `Dmax` that is positive-but-implausibly-low for a leader is *not*
/// rejected here (stock/development vary); the caller warns via
/// [`MIN_PLAUSIBLE_REFERENCE_DMAX`]. The plausibility check is **per-channel**: a
/// colored/wrong region can average to a plausible gray density while one channel
/// sits barely above the base (essentially unexposed), so [`ReferenceDmax`] carries
/// the per-channel densities alongside the scalar for the caller to test the
/// weakest channel — a genuine leader is dense in *every* channel, not just on the
/// gray average.
pub(crate) fn reference_dmax(reference: [f32; 3], base: &FilmBase) -> Result<ReferenceDmax> {
    let base = [base.r, base.g, base.b];
    let mut per_channel = [0.0f32; 3];
    for (c, name) in ["red", "green", "blue"].into_iter().enumerate() {
        let t = reference[c];
        // (a) An effectively-zero / non-finite reference channel is a degenerate
        // sample, not a leader — hard error rather than let the floor manufacture a
        // density (the Dmin "dark holder → zero channel" gotcha, applied to Dmax).
        if !t.is_finite() || t <= SCAN_EPSILON {
            return Err(NcError::Other(format!(
                "reference Dmax: the {name} channel transmission ({t}) is non-finite or \
                 at/below the scan floor ({SCAN_EPSILON}) — an effectively-zero / clipped \
                 sample (dead sensor, clipped black, or the dark holder beside the leader), \
                 not a fully-exposed reference; sample the light-struck roll leader's \
                 interior, or pass an explicit --d-max"
            )));
        }
        // (b) Validate density per channel before the gray reduction: a leader is
        // denser than the base on every channel. A colored/wrong region can average
        // positive while one channel out-transmits the base — reject it loudly.
        let d = -(t / base[c]).log10();
        if !d.is_finite() || d <= 0.0 {
            return Err(NcError::Other(format!(
                "reference Dmax: the {name} channel density ({d}) is not positive — the \
                 region out-transmits the film base on this channel, so it is not a \
                 fully-exposed reference (a leader is denser than the base in every \
                 channel); sample the light-struck roll leader's interior, or pass an \
                 explicit --d-max"
            )));
        }
        per_channel[c] = d;
    }
    // Each channel is finite and `> 0`, so the gray mean is finite and `> 0` too
    // (three finite positives, bounded well below overflow: `t > SCAN_EPSILON` and
    // `base ≤ 1` cap each `D` near `6`).
    let scalar = (per_channel[0] + per_channel[1] + per_channel[2]) / 3.0;
    debug_assert!(
        scalar.is_finite() && scalar > 0.0,
        "per-channel guards ensure this"
    );
    Ok(ReferenceDmax {
        scalar,
        per_channel,
    })
}

/// Cap on how many density samples the `Auto` anchor examines. A 99.5th
/// percentile over ~1M samples is statistically indistinguishable from the full
/// population for anchoring purposes, and the cap bounds the measuring pass to a
/// ~4 MB transient buffer instead of a second image-sized allocation on large
/// scans (the curve itself applies in-place).
const AUTO_DMAX_MAX_SAMPLES: usize = 1 << 20;

/// Deterministic sampling stride for a density buffer of `len` samples: the
/// smallest stride that keeps the sample count under [`AUTO_DMAX_MAX_SAMPLES`],
/// bumped off multiples of 3 — the buffer is interleaved RGB, so a stride
/// divisible by 3 would sample a single channel and bias the pooled percentile.
fn auto_dmax_stride(len: usize) -> usize {
    let stride = len.div_ceil(AUTO_DMAX_MAX_SAMPLES).max(1);
    if stride > 1 && stride.is_multiple_of(3) {
        stride + 1
    } else {
        stride
    }
}

/// The [`AUTO_DMAX_PERCENTILE`] of the finite corrected densities, by nearest-rank
/// over a deterministic strided sample (see [`auto_dmax_stride`]).
///
/// Non-finite densities (`NaN` from corrupt/overflowed input) are excluded rather
/// than ranked, so a stray non-finite pixel can't become the anchor. An empty /
/// all-non-finite buffer yields `0.0` — a neutral anchor rather than a panic; the
/// encoder's non-finite counter still surfaces the underlying fault.
/// Uses `select_nth_unstable` (O(n)) — the returned order-statistic value is
/// independent of tie ordering — and a fixed stride derived only from the buffer
/// length, so the result stays deterministic: same buffer ⇒ same anchor.
fn auto_dmax(density: &DensityImage, region: Option<[u32; 4]>) -> f32 {
    match region {
        // No region → the whole interleaved buffer, on exactly the walk this
        // function has always used. Kept as its own arm rather than expressed as a
        // full-frame rectangle so the unrestricted result stays *bit-identical*: a
        // programmatic `Auto` with no region must not move because a region
        // parameter was added.
        None => auto_dmax_strided(&density.density),
        Some(rect) => auto_dmax_over(collect_region_samples(density, rect)),
    }
}

/// [`AUTO_DMAX_PERCENTILE`] of the finite densities in `densities`, by nearest
/// rank over the deterministic stride [`auto_dmax_stride`] defines.
fn auto_dmax_strided(densities: &[f32]) -> f32 {
    let stride = auto_dmax_stride(densities.len());
    let mut finite: Vec<f32> = Vec::with_capacity(densities.len().div_ceil(stride));
    finite.extend(
        densities
            .iter()
            .step_by(stride)
            .copied()
            .filter(|v| v.is_finite()),
    );
    auto_dmax_over(finite)
}

/// The percentile core: nearest-rank [`AUTO_DMAX_PERCENTILE`] of an
/// already-collected, already-finite-filtered sample.
///
/// An empty sample yields `0.0` — a neutral anchor rather than a panic; the
/// encoder's non-finite counter still surfaces the underlying fault.
/// `select_nth_unstable` is O(n) and the returned order statistic is independent
/// of tie ordering, so the result is a function of the sample alone.
fn auto_dmax_over(mut finite: Vec<f32>) -> f32 {
    if finite.is_empty() {
        return 0.0;
    }
    let rank = ((finite.len() - 1) as f32 * AUTO_DMAX_PERCENTILE).round() as usize;
    let (_, nth, _) = finite.select_nth_unstable_by(rank, f32::total_cmp);
    *nth
}

/// The finite corrected densities inside `rect`, on the same deterministic stride
/// the whole-frame walk uses.
///
/// A region **restricts the walk** rather than adding one: the stride is taken over
/// the samples actually visited, so the sample budget and the ~4 MB transient
/// buffer are unchanged and `pipeline::memory` owes this no new term.
///
/// Why a region at all: the opaque film holder sits at the [`SCAN_EPSILON`] floor,
/// so its corrected density is enormous and it **owns** the top percentile of a
/// whole-frame read — measured resolving 2.23-2.37 against a roll `Dmax` of
/// 1.28-1.38, with every frame rendering black
/// (`algo/auto-anchor-interior-measurement`).
///
/// `rect` is clamped to the frame rather than trusted: it is resolved from the
/// *scan*, and a caller that hands over one computed for different dimensions must
/// not index out of bounds.
fn collect_region_samples(density: &DensityImage, rect: [u32; 4]) -> Vec<f32> {
    let (w, h) = (density.width as usize, density.height as usize);
    let [x, y, rw, rh] = {
        let [rx, ry, cw, ch] = rect;
        let (x, y) = ((rx as usize).min(w), (ry as usize).min(h));
        [x, y, (cw as usize).min(w - x), (ch as usize).min(h - y)]
    };
    let visited = rw * rh * 3;
    let stride = auto_dmax_stride(visited);
    let mut finite: Vec<f32> = Vec::with_capacity(visited.div_ceil(stride));
    // One `step_by` over the region's rows chained end to end, so the stride keeps a
    // single phase across row boundaries and the visited set is a function of the
    // rectangle and the dimensions alone.
    finite.extend(
        (y..y + rh)
            .flat_map(|row| {
                let start = (row * w + x) * 3;
                density.density[start..start + rw * 3].iter()
            })
            .step_by(stride)
            .copied()
            .filter(|v| v.is_finite()),
    );
    finite
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
    use crate::types::{ExponentialParams, Reconstruction};

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    /// A 1×1 RGB image (optionally with a 1-sample IR plane) for pixel-math tests.
    fn pixel(rgb: [f32; 3], ir: Option<f32>) -> LinearImage {
        LinearImage::new(1, 1, rgb.to_vec(), ir.map(|v| vec![v])).unwrap()
    }

    /// The exponential curve carrying `gamma`, at its default placement.
    fn exponential(gamma: f32) -> DensityCurve {
        DensityCurve::Exponential(ExponentialParams {
            gamma,
            anchor: AnchorPlacement::WhiteAtDmax,
        })
    }

    /// The result of a full density-path reconstruction.
    #[derive(Debug)]
    struct Converted {
        out: LinearImage,
        dmax: Option<f32>,
        balance_range: Option<[f32; 2]>,
    }

    /// Run the full density path through the public entry point, `reconstruct`
    /// (stages 1–3).
    fn run(
        img: &LinearImage,
        base: &FilmBase,
        density: DensityParams,
        curve: DensityCurve,
        dmax: DmaxSource,
    ) -> Result<Converted> {
        let config = Reconstruction { density, curve };
        let (film, rep) = reconstruct_config(img, base, &config, DmaxInput::new(dmax))?;
        Ok(Converted {
            out: film.into_linear(),
            dmax: rep.dmax,
            balance_range: rep.balance_range,
        })
    }

    /// The anchored exponential curve on a prepared density buffer (stage 3) — the
    /// same composition `reconstruct`'s exponential arm performs.
    fn render(density: DensityImage, gamma: f32, dmax: Option<f32>) -> LinearImage {
        let anchor = dmax.unwrap_or(0.0);
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
        let out = render(d, 1.0, None);
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
        let out = render(d, 0.5, None);
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
        let out = render(d, 1.0, None);
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
        let via_config = run(
            &img,
            &base,
            density.clone(),
            exponential(gamma),
            DmaxSource::Auto,
        )
        .unwrap();
        let dimg = to_density(&img, &base, &density);
        let anchor = resolve_dmax(&dimg, DmaxInput::new(DmaxSource::Auto));
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
            DmaxSource::Fixed,
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
            run(
                &img,
                &base,
                params,
                DensityCurve::default(),
                DmaxSource::Fixed,
            )
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
            DmaxSource::Fixed,
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
                DmaxSource::Fixed,
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
                DmaxSource::Fixed,
            )
            .is_ok()
        );
    }

    #[test]
    fn anchored_exponent_survives_extreme_gamma_and_dmax() {
        // Regression (PR review): the anchor used to be a separate 10^(−γ·Dmax)
        // gain, so γ·D' alone could overflow f32 before the gain cancelled it
        // (γ = 5, D' = 8 ⇒ 10^40 = inf ⇒ scene white rendered inf/NaN). With the
        // anchored exponent, D' = Dmax maps to exactly 1.0 regardless of scale.
        let gamma = 5.0f32;
        let dmax = 8.0f32;
        let dimg = DensityImage {
            width: 1,
            height: 1,
            density: vec![dmax, dmax, dmax],
            ir: None,
        };
        let out = render(dimg, gamma, Some(dmax));
        for v in &out.rgb {
            assert!(v.is_finite(), "overflowed: {v}");
            assert!(approx(*v, 1.0, 1e-5), "scene white should be 1.0, got {v}");
        }
    }

    #[test]
    fn auto_dmax_stride_is_bounded_and_channel_unbiased() {
        // Small buffers are sampled exhaustively.
        assert_eq!(auto_dmax_stride(0), 1);
        assert_eq!(auto_dmax_stride(3 * 100), 1);
        assert_eq!(auto_dmax_stride(AUTO_DMAX_MAX_SAMPLES), 1);
        // Large buffers are strided to stay under the cap...
        let big = 10 * AUTO_DMAX_MAX_SAMPLES;
        let stride = auto_dmax_stride(big);
        assert!(big.div_ceil(stride) <= AUTO_DMAX_MAX_SAMPLES + 1);
        // ...and the stride is never a multiple of 3 (interleaved RGB — a
        // 3-divisible stride would sample one channel only).
        for len in [
            big,
            3 * AUTO_DMAX_MAX_SAMPLES,
            6 * AUTO_DMAX_MAX_SAMPLES + 5,
        ] {
            let s = auto_dmax_stride(len);
            assert!(
                s == 1 || !s.is_multiple_of(3),
                "len {len}: stride {s} is 3-divisible"
            );
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
            DmaxSource::Fixed,
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
    fn auto_dmax_is_measured_after_the_regional_balance() {
        // Ordering contract (module doc): with `dmax = auto` the display-white
        // anchor is resolved from the *post-balance* densities, so it tracks
        // what is actually rendered. A uniform balance (+0.5 on every channel
        // at every tone) shifts every density by +0.5, so the reported Auto
        // anchor must shift by the same amount versus the neutral run.
        let base = FilmBase::from([0.6, 0.6, 0.6]);
        let img = LinearImage::new(2, 1, vec![0.5, 0.5, 0.5, 0.05, 0.05, 0.05], None).unwrap();
        // Explicit `Auto`: the default is the roll-fixed `Fixed` anchor, which
        // ignores the buffer — this test pins the `Auto` measurement's
        // post-balance ordering, so it must opt into `Auto`.
        let rep_neutral = run(
            &img,
            &base,
            DensityParams::default(),
            exponential(1.0),
            DmaxSource::Auto,
        )
        .unwrap();
        let rep_balanced = run(
            &img,
            &base,
            DensityParams {
                shadow_balance: [0.5, 0.5, 0.5],
                highlight_balance: [0.5, 0.5, 0.5], // tone-independent +0.5
                ..DensityParams::default()
            },
            exponential(1.0),
            DmaxSource::Auto,
        )
        .unwrap();
        let (a, b) = (rep_neutral.dmax.unwrap(), rep_balanced.dmax.unwrap());
        assert!(
            approx(b - a, 0.5, 1e-5),
            "auto dmax must be measured post-balance: neutral {a}, balanced {b}"
        );
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
            DmaxSource::Fixed,
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
            DmaxSource::Fixed,
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
            DmaxSource::Fixed,
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
        let a = run(
            &img,
            &base,
            params.clone(),
            DensityCurve::default(),
            DmaxSource::Fixed,
        )
        .unwrap()
        .out;
        let b = run(
            &img,
            &base,
            params,
            DensityCurve::default(),
            DmaxSource::Fixed,
        )
        .unwrap()
        .out;
        assert_eq!(a.rgb, b.rgb);
        assert_eq!(a.ir.as_deref(), Some(&[0.1f32, 0.2, 0.3][..]));
    }

    // --- Dmax white anchor -----------------------------------------------------

    #[test]
    fn none_anchor_is_bit_exact_with_pre_anchor_render() {
        // A `None` anchor must reproduce the unanchored render bit-for-bit: the
        // anchor term is exactly 0.0 and `d − 0.0 == d` for every f32, so every
        // output sample must equal the direct pre-anchor arithmetic to the bit
        // (HDR f32 workflows depend on this). Uses `assert_eq!`, not an epsilon.
        let density = vec![0.7f32, -0.3, 1.2, 0.0, 2.0, -1.1];
        let dimg = DensityImage {
            width: 2,
            height: 1,
            density: density.clone(),
            ir: None,
        };
        let gamma = 1.3;
        assert_eq!(
            resolve_dmax(&one_row(&density), DmaxInput::new(DmaxSource::None)),
            None,
            "no anchor resolved for None"
        );
        let out = render(dimg, gamma, None);
        for (i, &d) in density.iter().enumerate() {
            assert_eq!(
                out.rgb[i],
                10f32.powf(gamma * d),
                "sample {i} not bit-exact"
            );
        }
    }

    #[test]
    fn explicit_anchor_maps_that_density_to_display_white() {
        // With a neutral print, the pixel at `D' = Dmax` (scene white) renders to
        // exactly 1.0, and the base (`D' = 0`) to `10^(−γ·Dmax) < 1` (near black).
        let dmax = 1.5f32;
        let gamma = 2.0f32;
        let dimg = DensityImage {
            width: 2,
            height: 1,
            density: vec![dmax, dmax, dmax, 0.0, 0.0, 0.0],
            ir: None,
        };
        assert_eq!(
            resolve_dmax(&dimg, DmaxInput::new(DmaxSource::Explicit(dmax))),
            Some(dmax)
        );
        let out = render(dimg, gamma, Some(dmax));
        for c in 0..3 {
            assert!(
                approx(out.rgb[c], 1.0, 1e-5),
                "scene white → 1.0 (chan {c})"
            );
            assert!(approx(out.rgb[3 + c], 10f32.powf(-gamma * dmax), 1e-6));
            assert!(out.rgb[3 + c] < 1.0, "base below white (chan {c})");
        }
    }

    #[test]
    fn auto_dmax_high_percentile_resists_outliers() {
        // 200 samples at 1.0 plus one blown 1000.0 (< 0.5% of the data): the
        // 99.5th percentile stays on the bulk value, not the specular/dust outlier.
        let mut d = vec![1.0f32; 200];
        d.push(1000.0);
        assert!(
            approx(auto_dmax_strided(&d), 1.0, 1e-6),
            "got {}",
            auto_dmax_strided(&d)
        );
    }

    #[test]
    fn auto_dmax_nearest_rank_matches_the_percentile_index() {
        // Distinct values pin the exact nearest-rank index `round((n−1)·p)` in both
        // directions (a constant-bulk test would pass for any rank ≤ the top).
        // 1000 values 0..=999: index = round(999·0.995) = round(994.005) = 994.
        let d: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        assert_eq!(auto_dmax_strided(&d), 994.0);
    }

    #[test]
    fn auto_dmax_ignores_non_finite() {
        // Non-finite densities are excluded from the rank, never returned.
        let d = vec![f32::NAN, 0.5, f32::INFINITY, 0.5, f32::NEG_INFINITY, 0.5];
        assert!(approx(auto_dmax_strided(&d), 0.5, 1e-6));
        // All-non-finite / empty → 0.0 neutral fallback (gain 1.0), not a panic.
        assert_eq!(auto_dmax_strided(&[f32::NAN, f32::INFINITY]), 0.0);
        assert_eq!(auto_dmax_strided(&[]), 0.0);
    }

    #[test]
    fn auto_anchor_is_deterministic() {
        // Same input + params ⇒ identical output (the determinism contract).
        let base = FilmBase::from([0.6, 0.3, 0.18]);
        let img = LinearImage::new(
            3,
            1,
            vec![0.5, 0.25, 0.15, 0.3, 0.15, 0.09, 0.1, 0.05, 0.03],
            None,
        )
        .unwrap();
        let a = run(
            &img,
            &base,
            DensityParams::default(),
            exponential(1.0),
            DmaxSource::Auto,
        )
        .unwrap()
        .out;
        let b = run(
            &img,
            &base,
            DensityParams::default(),
            exponential(1.0),
            DmaxSource::Auto,
        )
        .unwrap()
        .out;
        assert_eq!(a.rgb, b.rgb);
    }

    #[test]
    fn reconstruct_surfaces_the_resolved_anchor() {
        let base = FilmBase::from([0.6, 0.6, 0.6]);
        let img = pixel([0.2, 0.2, 0.2], None);

        // Explicit → reports exactly that value.
        let rep = run(
            &img,
            &base,
            DensityParams::default(),
            exponential(1.0),
            DmaxSource::Explicit(1.25),
        )
        .unwrap();
        assert_eq!(rep.dmax, Some(1.25));
        // The (explicit, default-neutral) gains are surfaced too.

        // None → no anchor reported.
        let rep = run(
            &img,
            &base,
            DensityParams::default(),
            exponential(1.0),
            DmaxSource::None,
        )
        .unwrap();
        assert_eq!(rep.dmax, None);

        // Auto → a finite measured anchor.
        let rep = run(
            &img,
            &base,
            DensityParams::default(),
            exponential(1.0),
            DmaxSource::Auto,
        )
        .unwrap();
        assert!(rep.dmax.is_some_and(f32::is_finite));

        // Fixed (the default) → the nominal roll-fixed anchor, reported verbatim.
        let rep = run(
            &img,
            &base,
            DensityParams::default(),
            DensityCurve::default(),
            DmaxSource::Fixed,
        )
        .unwrap();
        assert_eq!(rep.dmax, Some(NOMINAL_DMAX));
    }

    #[test]
    fn auto_anchor_maps_measured_scene_white_to_display_white() {
        // Identity gain: this asserts *every* channel lands on 1.0, which is a statement
        // about the anchor. The default per-channel gain makes the three densities differ,
        // so only one channel could — a different property, pinned elsewhere.
        // End-to-end: a uniform-density image has one density value, so the auto
        // percentile equals it and the curve maps it to display white ≈ 1.0. Ties
        // the measured percentile to the curve gain (catches an anchor sign error
        // the explicit test's separate path could miss).
        let gamma = 1.8f32;
        let base = FilmBase::from([0.8, 0.8, 0.8]);
        let img = LinearImage::new(4, 1, vec![0.2f32; 12], None).unwrap(); // scan < base ⇒ D > 0
        let dimg = to_density(&img, &base, &identity_gain());
        let resolved = resolve_dmax(&dimg, DmaxInput::new(DmaxSource::Auto));
        let out = render(dimg, gamma, resolved);
        let dmax = resolved.unwrap();
        assert!(
            dmax > 0.0,
            "measured scene-white density should be positive"
        );
        for v in &out.rgb {
            assert!(
                approx(*v, 1.0, 1e-4),
                "scene white → 1.0, got {v} (dmax {dmax})"
            );
        }
    }

    #[test]
    fn auto_anchor_is_a_scalar_pooled_across_channels() {
        // Channel-asymmetric densities (R high, B low): the anchor is a single
        // pooled scalar — the *same* gain on every channel — so it can't double as
        // color correction (that's the auto-WB modes' job). Prove the per-channel
        // ratio `out_c / 10^(γ·D'_c)` is identical across channels (== anchor gain).
        let dimg = DensityImage {
            width: 2,
            height: 1,
            density: vec![2.0, 1.0, 0.1, 2.0, 1.0, 0.1],
            ir: None,
        };
        let gamma = 1.0f32;
        let resolved = resolve_dmax(&dimg, DmaxInput::new(DmaxSource::Auto));
        let out = render(dimg.clone(), gamma, resolved);
        let dmax = resolved.unwrap();
        let gain = 10f32.powf(-gamma * dmax);
        for c in 0..3 {
            let expected = 10f32.powf(gamma * dimg.density[c]) * gain;
            assert!(
                approx(out.rgb[c], expected, 1e-4),
                "chan {c}: {}",
                out.rgb[c]
            );
        }
    }

    // --- Dmax: fixed nominal + roll-fixed reference ----------------------------

    /// A flat interleaved density buffer as a one-row [`DensityImage`], for the
    /// anchor-source tests that care about the *source* rather than the geometry.
    /// The length must be a multiple of 3 (one row of `len / 3` pixels).
    fn one_row(densities: &[f32]) -> DensityImage {
        assert_eq!(densities.len() % 3, 0, "interleaved RGB");
        DensityImage {
            width: (densities.len() / 3) as u32,
            height: 1,
            density: densities.to_vec(),
            ir: None,
        }
    }

    /// A frame with an opaque border: the `SCAN_EPSILON`-floor holder ring that
    /// owns the top percentile of a whole-frame read, plus a dim picture interior.
    /// Synthetic and committed, so the regression is caught with no external assets.
    fn frame_with_opaque_border(w: u32, h: u32, border: u32) -> DensityImage {
        let mut density = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let on_border = x < border || y < border || x >= w - border || y >= h - border;
                // The holder's corrected density is enormous (it blocked all light);
                // the picture sits well below the roll's real Dmax.
                let d = if on_border { 2.4 } else { 0.9 };
                density.extend_from_slice(&[d, d, d]);
            }
        }
        DensityImage {
            width: w,
            height: h,
            density,
            ir: None,
        }
    }

    #[test]
    fn a_region_keeps_the_opaque_border_out_of_the_auto_anchor() {
        // The defect this wiring exists to fix: `Auto` over the whole frame is owned
        // by the holder. Measured on the fixture rolls it resolved 2.23-2.37 against
        // a roll Dmax of 1.28-1.38, rendering every frame black
        // (`algo/auto-anchor-interior-measurement`).
        let dimg = frame_with_opaque_border(200, 200, 20);

        let whole = resolve_dmax(&dimg, DmaxInput::new(DmaxSource::Auto)).unwrap();
        assert!(
            whole > 2.0,
            "falsifiability: the whole-frame read must still be contaminated, got {whole}"
        );

        // The same frame measured over its interior lands in the picture.
        let inside = resolve_dmax(
            &dimg,
            DmaxInput {
                source: DmaxSource::Auto,
                region: Some([20, 20, 160, 160]),
            },
        )
        .unwrap();
        assert!(
            (inside - 0.9).abs() < 1e-6,
            "the interior read must be the picture density, got {inside}"
        );
    }

    #[test]
    fn a_none_region_is_bit_identical_to_the_unrestricted_walk() {
        // The `None` arm is deliberately its own path rather than a full-frame
        // rectangle, so that adding the region parameter moved no existing result.
        //
        // Pinned against a **literal** captured from the pre-region code (`2.4`, the
        // holder density this fixture's border carries), not against
        // `auto_dmax_strided`: the `None` arm *is* a call to that function, so
        // comparing the two asserts nothing and would stay green if a refactor
        // re-expressed `None` as a full-frame rectangle. The frame's densities are
        // exact f32 literals and the percentile is an order statistic, so there is
        // no transcendental in the chain and the bits are the same on every target.
        let dimg = frame_with_opaque_border(64, 48, 5);
        assert_eq!(
            resolve_dmax(&dimg, DmaxInput::new(DmaxSource::Auto))
                .unwrap()
                .to_bits(),
            2.4f32.to_bits(),
            "the unrestricted result must still be the pre-region walk's value"
        );

        // And the full-frame *rectangle* agrees, which is the invariant that makes
        // the separate arm an optimization rather than a second behaviour.
        assert_eq!(
            resolve_dmax(
                &dimg,
                DmaxInput {
                    source: DmaxSource::Auto,
                    region: Some([0, 0, 64, 48])
                }
            )
            .unwrap()
            .to_bits(),
            2.4f32.to_bits()
        );
    }

    #[test]
    fn a_region_is_clamped_to_the_frame_rather_than_trusted() {
        // A region resolved for different dimensions must not index out of bounds.
        let dimg = frame_with_opaque_border(40, 30, 3);
        for rect in [
            [0, 0, 10_000, 10_000],
            [39, 29, 100, 100],
            [10_000, 10_000, 5, 5],
            [0, 0, 0, 0],
        ] {
            let got = resolve_dmax(
                &dimg,
                DmaxInput {
                    source: DmaxSource::Auto,
                    region: Some(rect),
                },
            );
            assert!(got.is_some(), "no panic and an anchor for {rect:?}");
        }
    }

    #[test]
    fn a_region_does_not_enlarge_the_sample_budget() {
        // The region restricts the walk rather than adding one, which is why
        // `pipeline::memory` owes this no term. A region can never visit more
        // samples than the frame does.
        let dimg = frame_with_opaque_border(300, 200, 10);
        let full = collect_region_samples(&dimg, [0, 0, 300, 200]).len();
        let part = collect_region_samples(&dimg, [10, 10, 280, 180]).len();
        assert!(part <= full, "region {part} must not exceed frame {full}");
        assert!(full <= AUTO_DMAX_MAX_SAMPLES, "the cap still holds: {full}");
    }

    #[test]
    fn fixed_anchor_resolves_to_the_nominal_constant() {
        // The default `Fixed` anchor is scene-independent: it ignores the buffer
        // and always resolves to NOMINAL_DMAX, so it is roll-fixed (every frame
        // gets the same anchor), unlike `Auto`.
        assert_eq!(
            resolve_dmax(&one_row(&[]), DmaxInput::new(DmaxSource::Fixed)),
            Some(NOMINAL_DMAX)
        );
        // A wildly different density distribution resolves to the same value.
        assert_eq!(
            resolve_dmax(
                &one_row(&[0.1, 0.2, 0.3]),
                DmaxInput::new(DmaxSource::Fixed)
            ),
            resolve_dmax(
                &one_row(&[5.0, 6.0, 7.0]),
                DmaxInput::new(DmaxSource::Fixed)
            )
        );
    }

    #[test]
    fn reference_dmax_is_the_gray_mean_of_base_relative_density() {
        // A near-opaque reference at transmission t against base b gives per
        // channel D = -log10(t/b); the scalar Dmax is their mean. Base = 1 so
        // D = -log10(t): t = [0.01, 0.001, 0.1] → D = [2, 3, 1] → mean 2.0.
        let base = FilmBase::from([1.0, 1.0, 1.0]);
        let d = reference_dmax([0.01, 0.001, 0.1], &base).unwrap().scalar;
        assert!(approx(d, 2.0, 1e-5), "got {d}");
        // Orange base: a neutral (per-channel-equal fraction of base) reference
        // yields equal per-channel densities, so the mean equals that density —
        // the scalar carries no per-channel (white-balance) term.
        let base = FilmBase::from([0.5, 0.25, 0.15]);
        let frac = 0.05f32; // reference transmits 5% of each channel's base
        let d = reference_dmax([0.5 * frac, 0.25 * frac, 0.15 * frac], &base)
            .unwrap()
            .scalar;
        assert!(approx(d, -(frac.log10()), 1e-5), "got {d}");
    }

    #[test]
    fn reference_dmax_rejects_a_non_opaque_region() {
        // A region that is *brighter* than the base on every channel (transmission
        // above base) yields a non-positive density on every channel — not a
        // fully-exposed reference. Fail loudly (exit 1), never a silently-wrong anchor.
        let base = FilmBase::from([0.3, 0.3, 0.3]);
        let err = reference_dmax([0.6, 0.6, 0.6], &base).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        // A non-finite reference sample also fails loudly rather than laundering.
        assert!(reference_dmax([f32::NAN, 0.01, 0.01], &base).is_err());
    }

    #[test]
    fn reference_dmax_rejects_a_floored_or_zero_channel() {
        // A channel at/below the SCAN_EPSILON floor (dead sensor, clipped black, or
        // the dark holder beside the leader) must NOT be laundered by the floor into
        // a huge density (≈ 6) that passes the positivity check and freezes a
        // black-rendering anchor — the Dmin "dark holder → zero channel" gotcha.
        let base = FilmBase::from([0.9, 0.55, 0.42]);
        // Exactly zero, and a subnormal below the floor: both hard-error.
        assert_eq!(
            reference_dmax([0.0, 0.02, 0.02], &base)
                .unwrap_err()
                .exit_code(),
            1
        );
        assert!(reference_dmax([0.02, SCAN_EPSILON, 0.02], &base).is_err());
        assert!(reference_dmax([0.02, 0.02, SCAN_EPSILON / 2.0], &base).is_err());
        // A negative transmission (noise) is degenerate too.
        assert!(reference_dmax([0.02, 0.02, -0.01], &base).is_err());
    }

    #[test]
    fn reference_dmax_rejects_a_per_channel_out_transmitting_region() {
        // A colored/wrong region can average to a *positive* gray density while one
        // channel out-transmits the base — the per-channel guard (before the gray
        // reduction) must still reject it. base = 1 so D = -log10(t):
        // t = [2, 0.1, 0.1] → D ≈ [-0.30, 1, 1], mean ≈ 0.57 > 0, but the red
        // channel out-transmits the base, so this is not a leader.
        let base = FilmBase::from([1.0, 1.0, 1.0]);
        let mean = ((-2.0f32.log10()) + 1.0 + 1.0) / 3.0;
        assert!(mean > 0.0, "the gray average alone would pass ({mean})");
        let err = reference_dmax([2.0, 0.1, 0.1], &base).unwrap_err();
        assert_eq!(err.exit_code(), 1, "per-channel guard must reject it");
    }

    #[test]
    fn reference_dmax_below_plausible_threshold_still_returns_for_the_caller_to_warn() {
        // A mid-tone region only somewhat denser than base (e.g. transmission ≈ 30%
        // of base → D ≈ 0.5) is a *valid* positive scalar but implausibly low for a
        // fully-exposed leader. `reference_dmax` returns it (thin stock varies); the
        // value sits below MIN_PLAUSIBLE_REFERENCE_DMAX so the CLI warns.
        let base = FilmBase::from([1.0, 1.0, 1.0]);
        let d = reference_dmax([0.3, 0.3, 0.3], &base).unwrap().scalar;
        assert!(d > 0.0 && d < MIN_PLAUSIBLE_REFERENCE_DMAX, "got {d}");
        // A genuine near-opaque leader clears the threshold.
        let d = reference_dmax([0.01, 0.01, 0.01], &base).unwrap().scalar;
        assert!(
            d >= MIN_PLAUSIBLE_REFERENCE_DMAX,
            "leader should clear: {d}"
        );
    }

    #[test]
    fn reference_dmax_exposes_a_weak_channel_a_plausible_scalar_hides() {
        // Codex's colored-region example: base [1,1,1], transmissions
        // ≈ [0.001, 0.99, 0.99] → per-channel densities ≈ [3.0, 0.004, 0.004].
        // The gray mean ≈ 1.0 clears MIN_PLAUSIBLE_REFERENCE_DMAX, yet green and
        // blue are essentially unexposed base — not a leader. The per-channel
        // densities expose the weak channels so the caller can warn on the minimum.
        let base = FilmBase::from([1.0, 1.0, 1.0]);
        let measured = reference_dmax([0.001, 0.99, 0.99], &base).unwrap();
        assert!(
            measured.scalar >= MIN_PLAUSIBLE_REFERENCE_DMAX,
            "the gray average alone would pass the plausibility check ({})",
            measured.scalar
        );
        let min_channel = measured
            .per_channel
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min);
        assert!(
            min_channel < MIN_PLAUSIBLE_REFERENCE_DMAX,
            "the weakest channel must be implausibly low ({min_channel})"
        );
        // The scalar is still the mean of the per-channel densities.
        let mean = measured.per_channel.iter().sum::<f32>() / 3.0;
        assert!(
            approx(measured.scalar, mean, 1e-6),
            "got {}",
            measured.scalar
        );
    }

    #[test]
    fn reference_derived_dmax_introduces_no_per_channel_correction() {
        // The dmax-reference core guarantee: a reference-derived `Dmax` is a plain
        // scalar, so it applies the *same* gain on every channel — a
        // reference-derived anchor and an equal explicit `--d-max` scalar render
        // identical color. Prove it directly: with the reference-derived anchor the
        // per-channel ratio `out_c / 10^(γ·D'_c)` is identical across channels
        // (== the anchor gain `10^(−γ·Dmax)`), so no channel is scaled differently.
        let base = FilmBase::from([0.6, 0.35, 0.2]);
        // A near-opaque reference: a few % of each channel's base (dense/neutral).
        let refl = [0.6 * 0.03, 0.35 * 0.03, 0.2 * 0.03];
        let d = reference_dmax(refl, &base).unwrap().scalar;
        assert!(
            d > 0.0,
            "reference Dmax should be a positive scalar, got {d}"
        );

        // Channel-asymmetric corrected densities so a hidden per-channel term
        // would show up as unequal ratios.
        let dimg = DensityImage {
            width: 2,
            height: 1,
            density: vec![2.0, 1.0, 0.1, 2.0, 1.0, 0.1],
            ir: None,
        };
        let gamma = 1.3f32;
        let out = render(dimg.clone(), gamma, Some(d));
        let gain = 10f32.powf(-gamma * d); // the single scalar anchor gain
        for c in 0..3 {
            let expected = 10f32.powf(gamma * dimg.density[c]) * gain;
            assert!(
                approx(out.rgb[c], expected, 1e-4),
                "chan {c}: {}",
                out.rgb[c]
            );
        }
    }
}
