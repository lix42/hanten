//! Reconstruction by inverting a film stock's published characteristic curve.
//!
//! # What this stage is
//!
//! The parametric curves ([`crate::algo::sigmoid`] and the exponential) *model* the
//! film's response with a slope and an anchor. This one **reads it**: each dye layer's
//! measured `density → log exposure` relation, digitized from the manufacturer's
//! characteristic curve, inverted per channel. The output is relative scene exposure with
//! mid-grey at 0.18 by construction, so there is no anchor to resolve and no contrast to
//! choose — both are properties of the curve the film actually has.
//!
//! This is the shape the standard film-scan pipelines use. ACES's `ADX → ACES` transform
//! is per-channel density → cross-channel matrix → per-channel curve inverse (a toe LUT
//! below a threshold, a straight line above it) → `10^` → matrix, with a *generic* film
//! model whose implied gamma is 0.55 and which places mid-grey 0.70 density above the film
//! base. Our per-stock measurements bracket both numbers (gamma 0.50–0.61, mid-grey
//! 0.54–0.70 above base), so this stage is that transform with the stock's own data
//! substituted for the generic model.
//!
//! # Why per-channel matters
//!
//! Every C-41 stock measured has a blue layer 12–19 % steeper than its red one, so a
//! *single* contrast applied to all three channels leaves a cast that grows with density —
//! measured at **+1.26 stops per unit corrected density** across 21 real frames, against
//! +1.29 predicted by the datasheets. Inverting each channel's own curve removes it by
//! construction: the residual is +0.09. See `algo::curve_probe` and the 2026-09-04 entries
//! in `docs/progress/algo.md`.
//!
//! # What it deliberately does not do
//!
//! No tone shaping. The film's own toe is *inverted* here (the film compresses shadows, so
//! recovering the scene expands them); any toe or shoulder the picture wants is display
//! character and belongs to `print.display_tone`, per
//! `algo/reconstruction-render-curve-split`. The measured stocks have no shoulder at all
//! within the density range a real scan occupies.

pub mod curves;

use rayon::prelude::*;

use curves::{STOCKS, StockCurves};

use crate::algo::FilmRgbImage;
use crate::algo::density::{DensityImage, apply_curve_per_channel};
use crate::types::{FilmStock, NcError, Result};

/// The curve set for a resolved stock.
///
/// Infallible: [`FilmStock`] is an enum whose every variant is generated alongside the
/// table, and `stocks_cover_every_film_stock_variant` pins that. An unknown *name* is
/// rejected at the CLI boundary, where the error can list the accepted spellings.
pub fn curves_for(stock: FilmStock) -> &'static StockCurves {
    let name = stock.as_str();
    STOCKS
        .iter()
        .find(|s| s.name == name)
        .expect("every FilmStock variant has a pinned curve set")
}

/// Relative log exposure for one channel's corrected density.
///
/// Returns `(log_exposure, within_table)`. Outside the table the end segment's slope
/// continues rather than clamping: clamping would fold every out-of-range sample onto one
/// exposure and silently invent a flat patch where the picture has detail — nc never
/// clamps quietly, so the extrapolation is reported.
///
/// The render **discards** the flag, deliberately: [`apply_curve`] counts out-of-table
/// samples in its own parallel reduction pass and then transforms the buffer with a plain
/// `Fn(usize, f32) -> f32`. Threading a count back out of the transform would mean a
/// reduction-carrying variant of `density::apply_curve_per_channel` to save one pass over
/// a stage that is not the render's cost centre. The flag is for callers that need the
/// fact per sample — `algo::curve_probe` and this module's own tests.
///
/// The table is strictly increasing in density, so the inverse is single-valued.
pub(crate) fn invert(table: &[(f32, f32)], d: f32) -> (f32, bool) {
    let last = table.len() - 1;
    if d <= table[0].1 {
        let ((x0, d0), (x1, d1)) = (table[0], table[1]);
        return (x0 + (d - d0) * (x1 - x0) / (d1 - d0), d >= table[0].1);
    }
    if d >= table[last].1 {
        let ((x0, d0), (x1, d1)) = (table[last - 1], table[last]);
        return (x1 + (d - d1) * (x1 - x0) / (d1 - d0), d <= table[last].1);
    }
    let mut lo = 0usize;
    let mut hi = last;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if table[mid].1 <= d {
            lo = mid
        } else {
            hi = mid
        }
    }
    let ((x0, d0), (x1, d1)) = (table[lo], table[hi]);
    (x0 + (d - d0) * (x1 - x0) / (d1 - d0), true)
}

/// How far each channel's densities fell outside its published table, as a fraction of
/// samples. Carried to the report so an out-of-range render is visible rather than
/// inferred from the picture.
///
/// **Read it as a whole-frame statistic, because that is what it is.** On a full-frame
/// scan the film holder and rebate surround the picture, and they are far denser than any
/// exposed image — measured at **5.2–7.2 % of the frame across twelve frames on four
/// rolls, of which 0.00 % lay inside the picture area**. So a few per cent here is the
/// scan's border, not the photograph, and it says nothing about the render.
///
/// It is also **not** a check on the declared stock: rendering one frame under the shipped
/// stock profiles moved this figure only between 5.75 % and 6.55 %, because every C-41
/// table ends within ~0.2 density of the others. A 30 % wrong film base moved it from
/// 6.00 % to 6.44 %. Diagnosing either fault needs the *interior* fraction, which needs a
/// holder-excluded measurement region — `algo/auto-anchor-interior-measurement` owns that
/// (an IR-measured holder cut, then a blind fractional inset; it does **not** detect the
/// rebate and crops nothing). Once that exists, an interior figure above ~0 is meaningful,
/// and this one can be dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct OutOfTable {
    /// Fraction of samples below each channel's table, `[r, g, b]`.
    pub below: [f32; 3],
    /// Fraction above, `[r, g, b]`. On a full-frame scan this is dominated by the holder.
    pub above: [f32; 3],
}

impl OutOfTable {
    /// The worst channel's **total** extrapolated fraction: `below[c] + above[c]`,
    /// maximized over the three channels.
    ///
    /// **The two directions are summed, and that is what makes the threshold reachable.**
    /// Taking the largest of the six directional figures let a channel that was 15 % below
    /// *and* 15 % above report 15 %, so a frame 30 % extrapolated in one channel never
    /// crossed the 20 % warning (`cli::OUT_OF_TABLE_WARN_FRACTION`) — silently on `roll`,
    /// whose frame entries carry no `reconstruction_result` for the raw figures to be read
    /// from. Below and above are disjoint sample sets, so their sum is still a fraction of
    /// the frame rather than a double count.
    pub fn worst(&self) -> f32 {
        (0..3)
            .map(|c| self.below[c] + self.above[c])
            .fold(0.0, f32::max)
    }
}

/// Stage 3 for the characteristic curve: corrected density → relative scene exposure.
///
/// A non-finite density is propagated as `NaN` rather than counted as out-of-table — it is
/// corrupt input, which `io::encode`'s non-finite counter already surfaces, and folding it
/// into a range statistic would hide it behind a plausible-looking percentage.
pub fn apply_curve(density: DensityImage, stock: FilmStock) -> Result<(FilmRgbImage, OutOfTable)> {
    let sc = curves_for(stock);
    let tables = sc.channels;

    let (w, h) = (density.width as usize, density.height as usize);
    let samples = (w * h).max(1) as f32;
    // Each channel's table endpoints, hoisted so the count below is one pass.
    let bounds: [(f32, f32); 3] =
        std::array::from_fn(|c| (tables[c][0].1, tables[c][tables[c].len() - 1].1));
    // **One parallel pass with a per-channel reduction**, not three serial ones. The
    // count used to walk the whole buffer once per channel, single-threaded, beside a
    // render whose transform is already `par_chunks_exact_mut` — ~224M sequential
    // iterations on a 74 MP scan. The counters are integers, so the reduction order
    // cannot change the reported fractions.
    let (below, above) = density
        .density
        .par_chunks_exact(3)
        .fold(
            || ([0u64; 3], [0u64; 3]),
            |(mut b, mut a), px| {
                for (c, (lo, hi)) in bounds.iter().enumerate() {
                    let d = px[c];
                    if !d.is_finite() {
                        continue;
                    }
                    if d < *lo {
                        b[c] += 1;
                    } else if d > *hi {
                        a[c] += 1;
                    }
                }
                (b, a)
            },
        )
        .reduce(
            || ([0u64; 3], [0u64; 3]),
            |(mut b, mut a), (rb, ra)| {
                for c in 0..3 {
                    b[c] += rb[c];
                    a[c] += ra[c];
                }
                (b, a)
            },
        );

    let film = apply_curve_per_channel(density, move |c, d| {
        if !d.is_finite() {
            return f32::NAN;
        }
        let (log_e, _) = invert(tables[c], d);
        10f32.powf(log_e)
    });

    Ok((
        film,
        OutOfTable {
            below: below.map(|n| n as f32 / samples),
            above: above.map(|n| n as f32 / samples),
        },
    ))
}

/// Decades between an 18 % grey card and a ~89 % paper white — `log10(0.89 / 0.18)`.
///
/// The interval the *Judging Negative Exposures* aim pair spans, and therefore the
/// interval any comparison against the curve has to use. Shared by
/// [`aim_red_scale`] and the sheet-consistency test rather than restated: they are the
/// same measurement read two ways, and a divergence between them would be invisible.
pub(crate) const AIM_SEPARATION_DECADES: f32 = 0.694;

/// The sheets whose two published halves disagree by too much for the aim table to
/// correct anything, with the measured reason.
///
/// **Not derived from a threshold.** The corpus contains sheets that disagree by 11 %
/// and are still usable (Ektar 100, Ultramax 400), so any cut-off separating those from
/// these two would be a number invented to fit the answer. These are named because the
/// inconsistency was established per sheet: both tabulate `Δ = 0.25` against their own
/// curves' ~0.36 rise (+44 %), which is a different kind of disagreement from a curve
/// read slightly steep.
const NO_USABLE_AIM_DELTA: &[&str] = &["portra-800", "ultramax-800"];

impl StockCurves {
    /// Density on one channel's published curve at relative log exposure `x`, linearly
    /// interpolated between table points and extrapolated from the end segment outside
    /// them — the forward direction of [`invert`].
    pub(crate) fn density_at(&self, channel: usize, x: f32) -> f32 {
        let t = self.channels[channel];
        let i = t.partition_point(|p| p.0 <= x).clamp(1, t.len() - 1);
        let ((x0, d0), (x1, d1)) = (t[i - 1], t[i]);
        d0 + (x - x0) * (d1 - d0) / (x1 - x0)
    }

    /// The published `Δ` (paper white − grey card, Status M red), or `None` when this
    /// sheet states none that can be used — the derived generic, which has no aim table
    /// at all, and the two 800-speed sheets in [`NO_USABLE_AIM_DELTA`].
    pub(crate) fn usable_aim_delta(&self) -> Option<f32> {
        if NO_USABLE_AIM_DELTA.contains(&self.name) {
            return None;
        }
        self.aims.map(|[grey, white]| white - grey)
    }
}

/// The red-channel `--density-scale` factor that reconciles a stock's published
/// characteristic curve with its own published aim table, or `None` when the sheet
/// states no usable `Δ` (see [`StockCurves::usable_aim_delta`]).
///
/// # What it corrects
///
/// A datasheet reports the film twice: the *Judging Negative Exposures* table gives the
/// grey-card and paper-white aim densities, and the characteristic curve gives density
/// against exposure. They are independent measurements, and on several sheets they
/// disagree — Ektar 100's curve rises 11 % more across the aims' own interval than its
/// aim table says. Scaling red brings the curve to what the aim table reports.
///
/// # The direction is a reciprocal, and getting it backwards doubles the error
///
/// The factor that scales the **table's** red density is `Δ / rise` (Ektar 0.898).
/// `--density-scale` multiplies the **scan's** density *before* the table is inverted,
/// so the flag takes its reciprocal, `rise / Δ` (Ektar 1.114) — which is what this
/// returns. The **direction** is measured, not argued:
/// `algo::curve_probe::scale_against_the_characteristic_curve` applies Ektar's two
/// candidate factors across 21 frames and finds the reciprocal takes the green–magenta
/// drift to +0.01 stop per unit density where the table-side number takes it to +0.72,
/// against +0.35 for applying nothing.
///
/// Read that probe for what it is: it hard-codes **Ektar's** pair, so it establishes the
/// direction and that the correction helps, not that a per-stock factor generalises —
/// which is why `--preset characteristic-aim` ships as a named option rather than a
/// candidate default. What pins *this* function against the recorded constants is
/// `aim_red_scale_reproduces_the_measured_constants`, which runs in CI without assets.
///
/// Equivalently this is `1 + error/100` for the disagreement
/// `aim_table_agrees_with_the_curve` reports, which is why the two share
/// [`AIM_SEPARATION_DECADES`] and [`StockCurves::usable_aim_delta`].
pub fn aim_red_scale(stock: FilmStock) -> Option<f32> {
    let sc = curves_for(stock);
    let tabulated = sc.usable_aim_delta()?;
    // The axis is built to put this stock's own mid-grey aim at `log10(0.18)`, so the
    // curve's rise is read from there across exactly the separation the aims span.
    // Interval-matched, not a local slope: a narrow window around mid-grey lands on a
    // local wiggle and reports an artefact (see `aim_table_agrees_with_the_curve`).
    let log18 = 0.18f32.log10();
    let from_curve = sc.density_at(0, log18 + AIM_SEPARATION_DECADES) - sc.density_at(0, log18);
    (tabulated > 0.0 && from_curve > 0.0).then_some(from_curve / tabulated)
}

/// Guard for a programmatic caller: the tables must be invertible.
///
/// The shipped tables are generated and asserted in tests, so this only fires if someone
/// edits a literal by hand into a non-monotone shape — which would make the inverse
/// multi-valued and the render silently non-deterministic in the affected range.
pub fn check_tables(stock: FilmStock) -> Result<()> {
    let sc = curves_for(stock);
    for (c, table) in sc.channels.iter().enumerate() {
        if table.len() < 8 {
            return Err(NcError::Other(format!(
                "film stock `{}` channel {c}: only {} curve points — too few to invert",
                sc.name,
                table.len()
            )));
        }
        if table[0].1 != 0.0 {
            return Err(NcError::Other(format!(
                "film stock `{}` channel {c}: the table must start at density 0 (the film \
                 base), not {}",
                sc.name, table[0].1
            )));
        }
        for w in table.windows(2) {
            if !(w[1].1 > w[0].1 && w[1].0 > w[0].0) {
                return Err(NcError::Other(format!(
                    "film stock `{}` channel {c}: the curve is not strictly increasing at \
                     {:?} → {:?}, so its inverse is not single-valued",
                    sc.name, w[0], w[1]
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        CharacteristicParams, DensityCurve, DensityCurveType, DensityParams, FilmBase, LinearImage,
        Reconstruction,
    };

    /// `generic-c41`'s own mid-grey aim above base, red — the averaged curve's, not the
    /// mean of the table below (0.617), which averages numbers rather than curves.
    const GENERIC_MID_ABOVE_BASE: f32 = 0.624;

    /// `mid aim − D-min` per stock, from the datasheets (progress log, 2026-09-04). These
    /// are *not* read by the render — the curve carries the placement — so this table
    /// exists only to prove the log-exposure axis was shifted correctly when the curves
    /// were generated, and to bound the fixed decode's hand-frozen `d`.
    const STOCK_MID_ABOVE_BASE: &[(&str, f32)] = &[
        ("ektar-100", 0.611),
        ("portra-160", 0.640),
        ("portra-400", 0.600),
        ("portra-160vc", 0.651),
        ("portra-400vc", 0.651),
        ("portra-800", 0.542),
        ("gold-200", 0.699),
        ("ultramax-400", 0.615),
        ("ultramax-800", 0.542),
    ];

    /// The stock's published curve read **forward** — log exposure → density — by the
    /// same linear interpolation [`invert`] runs backwards.
    ///
    /// The synthetic film model every test below builds its negatives with. Stating the
    /// round-trip property needs a forward model that is *not* the inverse under test.
    ///
    /// Delegates to [`StockCurves::density_at`], which `aim_red_scale` made runtime code,
    /// rather than carrying a second copy of the interpolation — two identical
    /// interpolators in one file is a second source of truth by construction. The
    /// round-trip property is unaffected: what is under test is `invert` / `apply_curve`,
    /// and this is still not that.
    fn forward(sc: &StockCurves, ch: usize, log_e: f32) -> f32 {
        sc.density_at(ch, log_e)
    }

    /// Every sheet's **own aim table** must agree with its **own curve**.
    ///
    /// The two halves of a datasheet are independent measurements of the same film: the
    /// *Judging Negative Exposures* table gives the grey-card and paper-white densities, and
    /// the characteristic curve gives density against exposure. A grey card and a paper white
    /// are `log10(0.89/0.18) ≈ 0.694` decades apart, so `Δ / 0.694` must equal the curve's own
    /// mid-scale slope. When it does not, the *sheet* disagrees with itself, and re-reading
    /// the artwork cannot fix it — verified by extracting Ektar through two independent paths
    /// (raw content stream and SVG), which agreed to 0.004.
    ///
    /// This test therefore documents the corpus rather than guarding the extraction: the two
    /// sheets that fail are named with their measured error, so the state of the data is
    /// visible in code instead of remembered.
    ///
    /// It is **not** a predictor of rendered colour. Portra 160 passes at +1% yet still shows
    /// a measured green residual of +0.48 stops/density on real scans — see the 2026-09-06
    /// entries in `docs/progress/algo.md` for what that points at.
    #[test]
    fn aim_table_agrees_with_the_curve() {
        // Sheets whose two halves disagree by more than 10%, with the measured error. Named
        // rather than skipped: an unexplained failure must not look like a tolerance choice.
        const KNOWN_INCONSISTENT: &[(&str, i32)] = &[
            ("ektar-100", 11),
            // Its curve rises 11% more than its own aim table says across the aims' own
            // separation, and its `γ_G/γ_R` is 1.002 against every other stock's 1.02-1.05
            // — i.e. this sheet draws red and green as nearly parallel, so it predicts
            // almost no green divergence (+0.22 stops/density) where the scans show the
            // most of any stock (+1.26). Its aim table is also digit-for-digit Portra
            // 400's, which a higher-contrast film should not share.

            // The opposite direction, and untested by eye — no roll of it in the fixtures.
            ("ultramax-400", -11),
        ];
        for sc in STOCKS {
            // Skips the derived generic (no aim table) and the two 800-speed sheets,
            // whose Δ is unusable — the same predicate `aim_red_scale` refuses on, so a
            // sheet can never be correctable by one and unchecked by the other.
            let Some(tabulated) = sc.usable_aim_delta() else {
                continue;
            };
            // Compare the two published quantities **over the same interval**: the curve's
            // own density rise across exactly the separation the aims span, starting at the
            // grey aim (which the axis is built to put at `log10(0.18)`).
            //
            // Not a local slope. A first version measured gamma over ±0.35 decade around
            // mid-grey and divided the tabulated Δ by 0.694; that made Ektar look 15% out
            // with a `γ_G/γ_R` of 0.970 — both artefacts of a narrow window landing on a
            // local wiggle in that one curve. Widening to ±0.5 decade moves Ektar's ratio
            // to 1.002 and leaves every other stock unchanged, which is how the artefact
            // was found. Interval-matched quantities have no such freedom.
            let log18 = 0.18f32.log10();
            let from_curve =
                sc.density_at(0, log18 + AIM_SEPARATION_DECADES) - sc.density_at(0, log18);
            let error_pct = (100.0 * (from_curve / tabulated - 1.0)).round() as i32;

            match KNOWN_INCONSISTENT.iter().find(|(n, _)| *n == sc.name) {
                Some((_, recorded)) => assert!(
                    (error_pct - recorded).abs() <= 1,
                    "{}: the recorded inconsistency moved from {recorded}% to {error_pct}% \
                     — re-check the sheet and update the record",
                    sc.name
                ),
                None => assert!(
                    error_pct.abs() <= 10,
                    "{}: the aim table gives Δ = {tabulated:.3} but its own curve rises \
                     {from_curve:.3} over the same interval ({error_pct}%). Either the sheet \
                     disagrees with itself — add it to KNOWN_INCONSISTENT with the measured \
                     figure — or the extraction is wrong.",
                    sc.name
                ),
            }
        }
    }

    /// The aim-matched red scale reproduces the constants the review set was rendered
    /// with, and refuses the sheets that state no usable `Δ`.
    ///
    /// Those three numbers were hand-carried in the preset-review generator while
    /// the derivation lived only in an `#[ignore]`d probe; this is what let them be
    /// deleted. They are the *flag* values (the reciprocal), so a direction flip fails
    /// here rather than shipping a correction that doubles the error it was meant to fix.
    #[test]
    fn aim_red_scale_reproduces_the_measured_constants() {
        for (stock, expected) in [
            (FilmStock::Ektar100, 1.114),
            (FilmStock::Portra160, 1.029),
            (FilmStock::Gold200, 0.955),
        ] {
            let got = aim_red_scale(stock).expect("a sheet with a usable aim table");
            assert!(
                (got - expected).abs() < 0.002,
                "{}: aim-matched red scale {got:.4}, recorded {expected:.3}",
                stock.as_str()
            );
        }
        // The derived generic has no aim table, and the two 800-speed sheets tabulate a
        // Δ their own curves contradict by +44 %. `characteristic-aim` refuses all three
        // rather than applying a confidently wrong correction.
        for stock in [
            FilmStock::GenericC41,
            FilmStock::Portra800,
            FilmStock::Ultramax800,
        ] {
            assert!(
                aim_red_scale(stock).is_none(),
                "{} states no usable aim delta but produced a scale",
                stock.as_str()
            );
        }
    }

    /// Which side of 1.0 the scale falls on is a fact about each sheet, not a constant —
    /// and it is the half a "just scale red down a bit" shortcut would get wrong.
    ///
    /// Gold 200's curve rises *less* than its aim table says, so its factor is below 1
    /// where Ektar's and Portra 160's are above. Pinned because the flag multiplies the
    /// scan's density: a sign convention that happened to work on the two stocks above 1
    /// would invert Gold's correction silently.
    #[test]
    fn the_scale_straddles_unity_across_the_corpus() {
        assert!(aim_red_scale(FilmStock::Gold200).unwrap() < 1.0);
        assert!(aim_red_scale(FilmStock::Ektar100).unwrap() > 1.0);
        // Every usable sheet stays within a correction plausible for a digitized curve;
        // a factor outside this means the extraction moved, not that the film did.
        for stock in FilmStock::ALL {
            if let Some(k) = aim_red_scale(*stock) {
                assert!(
                    (0.85..=1.20).contains(&k),
                    "{}: {k:.3} is too large a correction to be a sheet disagreement",
                    stock.as_str()
                );
            }
        }
    }

    /// `density_at` is the forward direction of [`invert`], so the two must round-trip.
    ///
    /// Guards the shared helper the aim derivation and the sheet-consistency test both
    /// read through: an off-by-one in its segment search would move every aim scale by a
    /// plausible-looking amount and nothing else would notice.
    #[test]
    fn density_at_is_the_inverse_of_invert() {
        for stock in FilmStock::ALL {
            let sc = curves_for(*stock);
            for channel in 0..3 {
                for step in 0..=10 {
                    let x = 0.18f32.log10() + (step as f32 - 5.0) * 0.18;
                    let d = sc.density_at(channel, x);
                    let (back, in_table) = invert(sc.channels[channel], d);
                    assert!(
                        in_table,
                        "{} ch{channel}: {x} left the table",
                        stock.as_str()
                    );
                    assert!(
                        (back - x).abs() < 1e-4,
                        "{} ch{channel}: {x:.4} -> density {d:.4} -> {back:.4}",
                        stock.as_str()
                    );
                }
            }
        }
    }

    /// The pinned literals must match the digitized extraction they were generated from.
    ///
    /// This is the audit half of the `pipeline/colorimetry/` pattern: `curves.json` is
    /// produced from the publications in `docs/datasheets/` by a script that needs poppler
    /// and is run by hand; this test needs neither poppler nor network, so CI checks the
    /// correspondence on every run. A hand-edited literal — or a regeneration that was
    /// never carried across — fails here instead of quietly moving pixels.
    #[test]
    fn curves_match_the_digitized_json() {
        let json: serde_json::Value =
            serde_json::from_str(include_str!("curves.json")).expect("curves.json parses");
        let object = json.as_object().expect("curves.json is an object");
        assert_eq!(
            object.len(),
            STOCKS.len(),
            "curves.json and the pinned table hold different stocks"
        );
        for sc in STOCKS {
            let entry = object
                .get(sc.name)
                .unwrap_or_else(|| panic!("{} is missing from curves.json", sc.name));
            assert_eq!(entry["publication"], sc.publication, "{}", sc.name);
            assert_eq!(entry["revision"], sc.revision, "{}", sc.name);
            match sc.aims {
                Some([grey, white]) => {
                    assert_eq!(
                        entry["aim_grey_red"].as_f64().unwrap() as f32,
                        grey,
                        "{}",
                        sc.name
                    );
                    assert_eq!(
                        entry["aim_white_red"].as_f64().unwrap() as f32,
                        white,
                        "{}",
                        sc.name
                    );
                }
                None => assert!(entry["aim_grey_red"].is_null(), "{}", sc.name),
            }
            for (c, channel) in ["R", "G", "B"].iter().enumerate() {
                let points = entry["channels"][channel]["points"]
                    .as_array()
                    .unwrap_or_else(|| panic!("{} channel {channel}: no points", sc.name));
                assert_eq!(
                    points.len(),
                    sc.channels[c].len(),
                    "{} channel {channel}: point count",
                    sc.name
                );
                for (i, (want, got)) in points.iter().zip(sc.channels[c]).enumerate() {
                    let (wx, wd) = (
                        want[0].as_f64().unwrap() as f32,
                        want[1].as_f64().unwrap() as f32,
                    );
                    assert_eq!(
                        (wx, wd),
                        *got,
                        "{} channel {channel} point {i} drifted from curves.json",
                        sc.name
                    );
                }
            }
        }
    }

    #[test]
    fn every_shipped_table_is_invertible() {
        for stock in FilmStock::ALL {
            check_tables(*stock).unwrap_or_else(|e| panic!("{}: {e}", stock.as_str()));
        }
    }

    #[test]
    fn stocks_cover_every_film_stock_variant() {
        for stock in FilmStock::ALL {
            let sc = curves_for(*stock);
            assert_eq!(sc.name, stock.as_str());
        }
        assert_eq!(
            STOCKS.len(),
            FilmStock::ALL.len(),
            "the pinned table and the enum have drifted apart"
        );
    }

    /// The axis convention: a stock's published mid-grey aim must invert to 0.18. This is
    /// what makes the curve self-anchoring, so it is the load-bearing property of the
    /// generated data.
    #[test]
    fn published_mid_grey_inverts_to_eighteen_percent() {
        for (name, mid_above_base) in STOCK_MID_ABOVE_BASE {
            let sc = STOCKS.iter().find(|s| s.name == *name).unwrap();
            let (log_e, ok) = invert(sc.channels[0], *mid_above_base);
            assert!(ok, "{name}: the mid aim fell outside its own table");
            let exposure = 10f32.powf(log_e);
            assert!(
                (exposure - 0.18).abs() < 0.006,
                "{name}: mid-grey inverts to {exposure:.4}, not 0.18"
            );
        }
    }

    /// The derived generic must sit inside the spread of the stocks it averages, or it is
    /// not an average — it is a ninth opinion.
    #[test]
    fn generic_sits_inside_the_measured_spread() {
        let generic = curves_for(FilmStock::GenericC41);
        let (log_e, _) = invert(generic.channels[0], GENERIC_MID_ABOVE_BASE);
        assert!(
            (10f32.powf(log_e) - 0.18).abs() < 0.01,
            "the generic's own mid-above-base must invert to 0.18"
        );
        // Mid-scale gamma, red: the per-stock measurements run 0.50–0.61.
        let d = |x: f32| forward(generic, 0, x);
        let log18 = 0.18f32.log10();
        let gamma = (d(log18 + 0.35) - d(log18 - 0.35)) / 0.7;
        assert!(
            (0.50..=0.61).contains(&gamma),
            "generic red gamma {gamma:.3} is outside the measured per-stock range"
        );
    }

    /// The fixed decode's `d` is this generic's mid aim, rounded — the provenance
    /// `algo::fixed::MID_ABOVE_BASE` claims, checked here because only this module's tests
    /// may read the datasheet figures. Moving the constant means restating its derivation
    /// there and here, not loosening this.
    #[test]
    fn the_fixed_decode_mid_is_the_generic_aim() {
        let d = crate::algo::fixed::MID_ABOVE_BASE;
        assert_eq!(
            d,
            (GENERIC_MID_ABOVE_BASE * 100.0).round() / 100.0,
            "the fixed decode's d is no longer the generic aim rounded"
        );

        // A convention for every stock, so it must sit inside what the stocks measure.
        let (lo, hi) = STOCK_MID_ABOVE_BASE
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), (_, m)| {
                (lo.min(*m), hi.max(*m))
            });
        assert!(
            (lo..=hi).contains(&d),
            "d {d} is outside the stocks' {lo}..={hi}"
        );
    }

    /// The load-bearing property, end to end: build a **neutral** exposure ramp, run it
    /// forward through the stock's own published curves to get the negative densities that
    /// film would record, then reconstruct. Every channel must come back to the exposure
    /// it started from — which is exactly what "a grey ramp stays grey" means, and what a
    /// single scalar contrast cannot do (blue is 12-19% steeper on every stock).
    ///
    /// A synthetic test, but not a tautological one: it fails if the tables are misaligned
    /// between channels, if the log-exposure axis is shifted per channel, or if the
    /// inversion picks the wrong segment.
    #[test]
    fn a_neutral_ramp_reconstructs_neutral_on_every_stock() {
        for stock in FilmStock::ALL {
            let sc = curves_for(*stock);
            // Six stops around mid-grey, inside every stock's plotted range.
            for step in 0..=10 {
                let log_e = 0.18f32.log10() + (step as f32 - 5.0) * 0.18;
                for ch in 0..3 {
                    let density = forward(sc, ch, log_e);
                    let (back, in_table) = invert(sc.channels[ch], density);
                    assert!(
                        in_table,
                        "{} ch{ch}: {log_e} left the table",
                        stock.as_str()
                    );
                    assert!(
                        (back - log_e).abs() < 1e-4,
                        "{} ch{ch}: exposure {log_e:.4} came back as {back:.4}",
                        stock.as_str()
                    );
                }
            }
        }
    }

    /// The same property stated the way the defect is visible: at a *fixed* neutral
    /// exposure the three channels record different densities, and reconstruction must
    /// map all three back to the same exposure. The pre-curve pipeline maps them with one
    /// shared contrast, which cannot.
    #[test]
    fn channels_reconverge_although_their_densities_differ() {
        let sc = curves_for(FilmStock::Portra400);
        let log_e = 0.18f32.log10() + 0.6; // two stops over mid
        let (dr, db) = (forward(sc, 0, log_e), forward(sc, 2, log_e));
        assert!(
            db - dr > 0.2,
            "the premise of this stage: blue records {db:.3} where red records {dr:.3}"
        );
        let (er, _) = invert(sc.channels[0], dr);
        let (eb, _) = invert(sc.channels[2], db);
        assert!(
            (er - eb).abs() < 1e-4,
            "two densities {dr:.3}/{db:.3} that are the same scene exposure reconstructed \
             to {er:.4}/{eb:.4}"
        );
    }

    #[test]
    fn inversion_is_exact_at_table_points() {
        for stock in FilmStock::ALL {
            for table in curves_for(*stock).channels {
                for &(x, d) in table.iter().skip(1) {
                    let (got, ok) = invert(table, d);
                    assert!(ok);
                    assert!(
                        (got - x).abs() < 1e-3,
                        "{}: inverting {d} gave {got}, expected {x}",
                        stock.as_str()
                    );
                }
            }
        }
    }

    /// `worst()` sums a channel's two directions, so a frame extrapolated equally at both
    /// ends can still cross the warning threshold.
    ///
    /// The regression: as a max over all six directional figures, 15 % below and 15 % above
    /// in one channel reported 15 % and stayed under `cli::OUT_OF_TABLE_WARN_FRACTION`,
    /// though 30 % of that channel was extrapolated. Nothing else surfaces it on `roll`.
    #[test]
    fn worst_is_a_channels_total_not_its_larger_direction() {
        let split = OutOfTable {
            below: [0.15, 0.0, 0.0],
            above: [0.15, 0.0, 0.0],
        };
        assert!(
            (split.worst() - 0.30).abs() < 1e-6,
            "expected the channel total, got {}",
            split.worst()
        );
        // 0.20 is `cli::OUT_OF_TABLE_WARN_FRACTION`, restated rather than imported: this
        // module owns the statistic, the CLI owns the threshold.
        assert!(split.worst() > 0.20, "the warning must now be reachable");

        // Still the *worst channel*, not a sum across channels: three channels at 8 %
        // each is an 8 % frame, not a 24 % one.
        let spread = OutOfTable {
            below: [0.08, 0.08, 0.08],
            above: [0.0; 3],
        };
        assert!((spread.worst() - 0.08).abs() < 1e-6);
        assert_eq!(OutOfTable::default().worst(), 0.0);
    }

    /// Out-of-table samples extrapolate along the end slope and say so, rather than
    /// clamping — a clamp would silently flatten highlight or shadow detail.
    #[test]
    fn out_of_table_extrapolates_and_reports() {
        let t = curves_for(FilmStock::Portra400).channels[0];
        let (below, ok_below) = invert(t, -0.05);
        let (above, ok_above) = invert(t, t[t.len() - 1].1 + 0.5);
        assert!(!ok_below && !ok_above);
        assert!(below < t[0].0);
        assert!(above > t[t.len() - 1].0);
    }

    /// Every stock's blue layer is steeper than its red one — the property that makes a
    /// single scalar contrast wrong, and the reason this stage exists. If a regenerated
    /// table ever loses it, the data is wrong, not the film.
    #[test]
    fn blue_is_steeper_than_red_on_every_stock() {
        for stock in FilmStock::ALL {
            let sc = curves_for(*stock);
            let log18 = 0.18f32.log10();
            let gamma = |ch: usize| {
                let at = |x: f32| forward(sc, ch, x);
                (at(log18 + 0.35) - at(log18 - 0.35)) / 0.7
            };
            let (r, b) = (gamma(0), gamma(2));
            assert!(
                b > r * 1.05,
                "{}: blue gamma {b:.3} is not meaningfully steeper than red {r:.3}",
                stock.as_str()
            );
        }
    }

    // --- the wiring, end to end -----------------------------------------------
    //
    // Everything above pins the *tables* and [`invert`]. The tests below run the shipped
    // `algo::reconstruct` — `to_density` → `check_tables` → [`apply_curve`] →
    // `FilmRgbImage` — because nothing else did: `algo/characteristic-curve-coverage` was
    // opened for exactly that gap.
    //
    // They assert properties, not captured bits, so they survive the ~1-ULP libm spread
    // across targets. The bit-exact half lives in `pipeline::stages::golden`.

    /// The per-channel film base these tests reconstruct against.
    ///
    /// Deliberately **not** neutral: stage 1 divides by the base per channel, and a
    /// shared scalar there is invisible against a grey base. (A channel permuted between
    /// the stages is caught by the three tables differing, not by this.)
    fn test_base() -> FilmBase {
        FilmBase::from([0.9, 0.55, 0.42])
    }

    /// The scan a film would hand the decoder for a given set of **corrected** densities,
    /// so each pixel arrives at the curve carrying exactly `want`.
    ///
    /// Synthesizing the *scan* rather than handing densities straight to the curve is what
    /// puts `to_density` inside the assertion, which is the whole point of this section.
    ///
    /// It inverts stage 1 and stage 2's **per-channel** half only
    /// (`D′ = scale·(−log10(s/base)) + offset`). Two preconditions follow, neither of
    /// which any caller here violates and both of which a new one easily could:
    ///
    /// - `regional_balance` is stage 2's other half and is **not** inverted, so every
    ///   caller must leave `shadow_balance` / `highlight_balance` neutral. A non-neutral
    ///   pair is a per-tone per-channel offset the synthesis does not undo.
    /// - `want` must stay under `−log10(SCAN_EPSILON/base_c)` — about 5.95 / 5.74 / 5.62
    ///   for R/G/B at [`test_base`] — above which `to_density`'s dead-pixel floor clamps
    ///   and the round trip stops being one.
    fn scan_for(params: &DensityParams, want: &[[f32; 3]]) -> LinearImage {
        let base = test_base();
        let base = [base.r, base.g, base.b];
        let mut rgb = Vec::with_capacity(want.len() * 3);
        for px in want {
            for c in 0..3 {
                rgb.push(base[c] * 10f32.powf(-(px[c] - params.offset[c]) / params.scale[c]));
            }
        }
        LinearImage::new(want.len() as u32, 1, rgb, None).expect("a one-row synthetic scan")
    }

    /// Reconstruct the scan carrying `want` through the characteristic curve, as
    /// `(per-pixel exposures, report)`.
    fn reconstruct_densities(
        stock: FilmStock,
        density: &DensityParams,
        want: &[[f32; 3]],
    ) -> (Vec<[f32; 3]>, crate::algo::ReconstructionReport) {
        let image = scan_for(density, want);
        let config = Reconstruction::Density {
            density: density.clone(),
            curve: DensityCurve::Characteristic(CharacteristicParams { stock }),
        };
        let (film, report) = crate::algo::reconstruct(
            &image,
            &test_base(),
            &config,
            crate::types::DmaxInput::default(),
        )
        .expect("the characteristic reconstruction must succeed");
        let (pixels, rest) = film.rgb().as_chunks::<3>();
        debug_assert!(rest.is_empty(), "an RGB buffer is a whole number of pixels");
        (pixels.to_vec(), report)
    }

    /// The gain this curve resolves for itself (`[1, 1, 1]`), taken from its single
    /// definition rather than restated — the parametric calibration would correct the
    /// stock's own per-channel structure a second time.
    fn characteristic_density() -> DensityParams {
        DensityParams {
            scale: DensityParams::default_scale_for(DensityCurveType::Characteristic),
            ..DensityParams::default()
        }
    }

    /// Relative tolerance for a full-chain round trip.
    ///
    /// The synthesis and the reconstruction are algebraic inverses, so what is left is f32
    /// rounding through `10^` / `log10` and the two interpolations — **measured at
    /// 4.8e-7** (8 ULPs) across all ten stocks. The bound keeps 20x of that, which no
    /// cross-target libm difference can consume (~1 ULP each), and stays three orders
    /// tighter than any real wiring fault, which moves values by percent.
    const ROUND_TRIP_TOL: f32 = 1e-5;

    /// Every channel of every reconstructed pixel came back to the exposure the ramp
    /// started from, within [`ROUND_TRIP_TOL`].
    fn assert_round_trip(label: &str, exposures: &[f32], out: &[[f32; 3]]) {
        // `zip` truncates, so a short render would silently assert nothing.
        assert_eq!(out.len(), exposures.len(), "{label}: pixel count");
        for (&log_e, got) in exposures.iter().zip(out) {
            let want = 10f32.powf(log_e);
            for (c, &got) in got.iter().enumerate() {
                let err = (got / want - 1.0).abs();
                assert!(
                    err < ROUND_TRIP_TOL,
                    "{label} ch{c}: exposure {want:.5} reconstructed as {got:.5} \
                     ({err:.2e} relative)"
                );
            }
        }
    }

    /// **The load-bearing property, through the shipped wiring.** Run a neutral exposure
    /// ramp forward through the stock's own published curves into the densities film
    /// would record, synthesize the scan carrying them, and reconstruct. Every channel
    /// must come back to the exposure it started from.
    ///
    /// `a_neutral_ramp_reconstructs_neutral_on_every_stock` asserts this of [`invert`]
    /// alone; this asserts it of the chain. It fails if stage 1's base division is shared
    /// instead of per-channel, if one table is applied across all three channels, if the
    /// channels are permuted between stages, or if the dispatch reaches another curve.
    ///
    /// What it deliberately does **not** cover: where the regional balance sits. The
    /// balances are neutral here (their default), so that pass is a no-op and reordering
    /// it is invisible — a non-neutral balance is a per-tone per-channel offset and would
    /// destroy the neutrality being asserted. It is shared with the parametric curves and
    /// pinned by their goldens.
    #[test]
    fn a_neutral_ramp_reconstructs_neutral_through_the_whole_chain() {
        for stock in FilmStock::ALL {
            let sc = curves_for(*stock);
            // Six stops around mid-grey — the same span as the table-only test, and
            // inside every stock's plotted range.
            let exposures: Vec<f32> = (0..=10)
                .map(|step| 0.18f32.log10() + (step as f32 - 5.0) * 0.18)
                .collect();
            let want: Vec<[f32; 3]> = exposures
                .iter()
                .map(|&log_e| std::array::from_fn(|c| forward(sc, c, log_e)))
                .collect();
            let (out, report) = reconstruct_densities(*stock, &characteristic_density(), &want);

            // Nothing here is extrapolated, so the round trip below measures the published
            // curve rather than its end slope.
            assert_eq!(
                report.out_of_table,
                Some(OutOfTable::default()),
                "{}: the ramp left the published table",
                stock.as_str()
            );

            assert_round_trip(stock.as_str(), &exposures, &out);

            // "A grey ramp stays grey", stated directly: the three channels recorded
            // measurably different densities and must land on one exposure.
            for got in &out {
                let lo = got.iter().copied().fold(f32::INFINITY, f32::min);
                let hi = got.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                // `hi / lo` is only a spread when `lo` is positive — a negative or zero
                // channel would make the ratio pass while the pixel is nonsense. Checked
                // here rather than relying on `assert_round_trip` having run first, which
                // is an ordering a later edit can undo in silence.
                assert!(
                    lo > 0.0 && hi / lo - 1.0 < ROUND_TRIP_TOL,
                    "{}: a neutral exposure reconstructed as {got:?}",
                    stock.as_str()
                );
            }
        }
    }

    /// The axis convention, through the wiring: a stock's **published** mid-grey aim
    /// density reconstructs to 0.18.
    ///
    /// This is what makes the curve self-anchoring, which is why the report resolves no
    /// reference and no anchor — asserted here beside it, since a curve that silently
    /// acquired one would still round-trip a ramp.
    #[test]
    fn the_published_mid_grey_reconstructs_to_eighteen_percent_through_the_whole_chain() {
        let log18 = 0.18f32.log10();
        for (name, mid_above_base) in STOCK_MID_ABOVE_BASE {
            let stock = FilmStock::parse(name).expect("STOCK_MID_ABOVE_BASE names shipped stocks");
            let sc = curves_for(stock);
            // Red carries the published aim; green and blue take the curve's own neutral
            // densities at mid-grey, so the pixel is a plausible grey card rather than a
            // red-only probe.
            let want = [[
                *mid_above_base,
                forward(sc, 1, log18),
                forward(sc, 2, log18),
            ]];
            let (out, report) = reconstruct_densities(stock, &characteristic_density(), &want);
            assert_eq!(
                report.dmax, None,
                "{name}: this curve resolves no reference"
            );
            assert_eq!(
                report.curve_anchor, None,
                "{name}: this curve places no anchor"
            );
            for (c, &got) in out[0].iter().enumerate() {
                assert!(
                    (got - 0.18).abs() < 0.006,
                    "{name} ch{c}: mid-grey reconstructed as {got}"
                );
            }
        }
    }

    /// Stages 1–2 are `scale·d + offset`, not `scale·(d + offset)`, and the chain must
    /// apply them in that order.
    ///
    /// The ramp test cannot see this: `characteristic` resolves the identity gain, and at
    /// `scale = 1` the two spellings agree. An explicit non-neutral pair separates them —
    /// the synthesis inverts the documented definition, so a correct chain still lands on
    /// neutral while a transposed one lands `offset·(scale − 1)` away in density.
    ///
    /// It doubles as the only assertion that this curve honours `density.scale` /
    /// `density.offset` at all: they are resolved per curve, and a dropped one would
    /// otherwise show up as nothing.
    #[test]
    fn the_chain_applies_the_density_gain_before_the_offset() {
        let stock = FilmStock::Portra400;
        let sc = curves_for(stock);
        let density = DensityParams {
            scale: [1.15, 0.9, 0.85],
            offset: [0.08, -0.06, -0.05],
            ..DensityParams::default()
        };
        // Falsifiability, machine-checked rather than asserted in prose: the transposed
        // spelling really does land far outside ROUND_TRIP_TOL at these values, so a green
        // run means the order is right and not that the two spellings coincide. At a
        // mid-scale gamma near 0.55 this much density is 2-5% of exposure.
        for c in 0..3 {
            let transposed_shift = density.offset[c] * (density.scale[c] - 1.0);
            assert!(
                transposed_shift.abs() > 0.005,
                "channel {c}: the two spellings differ by only {transposed_shift} density"
            );
        }

        let exposures: Vec<f32> = (0..=6)
            .map(|step| 0.18f32.log10() + (step as f32 - 3.0) * 0.25)
            .collect();
        let want: Vec<[f32; 3]> = exposures
            .iter()
            .map(|&log_e| std::array::from_fn(|c| forward(sc, c, log_e)))
            .collect();
        let (out, _) = reconstruct_densities(stock, &density, &want);
        assert_round_trip("explicit gain/offset", &exposures, &out);
    }

    /// The reported extrapolation fractions must describe the samples that were actually
    /// extrapolated.
    ///
    /// [`apply_curve`] counts in a **separate parallel reduction** from the transform that
    /// renders them, so the two can drift apart with every gate green — and nothing
    /// asserted these numbers, though they reach the JSON report and a
    /// `--strict`-promotable warning. Recounted here from [`invert`]'s own per-sample flag
    /// over the densities `to_density` produced, so the two passes are compared rather
    /// than one being restated.
    #[test]
    fn the_reported_out_of_table_fractions_match_the_rendered_samples() {
        let stock = FilmStock::Portra400;
        let sc = curves_for(stock);
        let density = characteristic_density();
        // Below the film base, three in-table tones, and past the densest published point
        // in every channel — the shape a full-frame scan has, where the holder sits above
        // every table.
        let want: Vec<[f32; 3]> = vec![
            [-0.30, -0.30, -0.30],
            [0.20, 0.25, 0.30],
            [0.62, 0.70, 0.78],
            [1.05, 1.20, 1.35],
            std::array::from_fn(|c| sc.channels[c][sc.channels[c].len() - 1].1 + 0.5),
        ];
        let (out, report) = reconstruct_densities(stock, &density, &want);
        let oot = report
            .out_of_table
            .expect("the characteristic curve reports its extrapolation");

        // Recount from the densities the render itself saw, not from `want`: a target
        // sitting a rounding step from a table endpoint would otherwise be counted on the
        // other side here than there.
        let densities =
            crate::algo::density::to_density(&scan_for(&density, &want), &test_base(), &density);
        let mut below = [0u32; 3];
        let mut above = [0u32; 3];
        for px in densities.density.as_chunks::<3>().0 {
            for c in 0..3 {
                if invert(sc.channels[c], px[c]).1 {
                    continue;
                }
                if px[c] < sc.channels[c][0].1 {
                    below[c] += 1;
                } else {
                    above[c] += 1;
                }
            }
        }
        // The vector's own shape, so a recount that matched two zeros would fail here.
        assert_eq!(
            below,
            [1, 1, 1],
            "the probe must extrapolate below every table"
        );
        assert_eq!(
            above,
            [1, 1, 1],
            "the probe must extrapolate above every table"
        );

        let samples = want.len() as f32;
        for c in 0..3 {
            assert_eq!(oot.below[c], below[c] as f32 / samples, "channel {c} below");
            assert_eq!(oot.above[c], above[c] as f32 / samples, "channel {c} above");
        }

        // Extrapolated, not clamped: the out-of-table samples must render outside the
        // exposures the table's own endpoints carry. A clamp would flatten them onto the
        // endpoint while these fractions still reported detail there.
        for (c, t) in sc.channels.iter().enumerate() {
            assert!(
                out[0][c] < 10f32.powf(t[0].0),
                "channel {c}: a below-table sample rendered at {}, not below {}",
                out[0][c],
                10f32.powf(t[0].0)
            );
            assert!(
                out[4][c] > 10f32.powf(t[t.len() - 1].0),
                "channel {c}: an above-table sample rendered at {}, not above {}",
                out[4][c],
                10f32.powf(t[t.len() - 1].0)
            );
        }
    }
}
