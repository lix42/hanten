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
/// resolved picture region — `film-base/auto-base-redesign` locates the rebate already,
/// and `algo/auto-anchor-interior-measurement` owns plumbing it through. Once that exists,
/// an interior figure above ~0 is meaningful, and this one can be dropped.
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

    /// `mid aim − D-min` per stock, from the datasheets (progress log, 2026-09-04). These
    /// are *not* read by the render — the curve carries the placement — so this table
    /// exists only to prove the log-exposure axis was shifted correctly when the curves
    /// were generated.
    const MID_ABOVE_BASE: &[(&str, f32)] = &[
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
        // Decades between an 18% grey card and a ~89% paper white.
        const AIM_SEPARATION_DECADES: f32 = 0.694;
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
        // The 800-speed sheets tabulate Δ = 0.25 against their own curves' ~0.36; that
        // inconsistency is established separately, so they state no usable Δ at all.
        const NO_USABLE_DELTA: &[&str] = &["portra-800", "ultramax-800"];

        for sc in STOCKS {
            let Some([grey, white]) = sc.aims else {
                continue; // the derived generic has no aim table
            };
            if NO_USABLE_DELTA.contains(&sc.name) {
                continue;
            }
            let table = sc.channels[0];
            let at = |x: f32| {
                let i = table
                    .partition_point(|p| p.0 <= x)
                    .clamp(1, table.len() - 1);
                let ((x0, d0), (x1, d1)) = (table[i - 1], table[i]);
                d0 + (x - x0) * (d1 - d0) / (x1 - x0)
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
            let tabulated = white - grey;
            let from_curve = at(log18 + AIM_SEPARATION_DECADES) - at(log18);
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
        for (name, mid_above_base) in MID_ABOVE_BASE {
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
        let (log_e, _) = invert(generic.channels[0], 0.624);
        assert!(
            (10f32.powf(log_e) - 0.18).abs() < 0.01,
            "the generic's own mid-above-base must invert to 0.18"
        );
        // Mid-scale gamma, red: the per-stock measurements run 0.50–0.61.
        let d = |x: f32| -> f32 {
            let t = generic.channels[0];
            let i = t.partition_point(|p| p.0 <= x).clamp(1, t.len() - 1);
            let ((x0, d0), (x1, d1)) = (t[i - 1], t[i]);
            d0 + (x - x0) * (d1 - d0) / (x1 - x0)
        };
        let log18 = 0.18f32.log10();
        let gamma = (d(log18 + 0.35) - d(log18 - 0.35)) / 0.7;
        assert!(
            (0.50..=0.61).contains(&gamma),
            "generic red gamma {gamma:.3} is outside the measured per-stock range"
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
            let forward = |ch: usize, log_e: f32| -> f32 {
                let t = sc.channels[ch];
                let i = t.partition_point(|p| p.0 <= log_e).clamp(1, t.len() - 1);
                let ((x0, d0), (x1, d1)) = (t[i - 1], t[i]);
                d0 + (log_e - x0) * (d1 - d0) / (x1 - x0)
            };
            // Six stops around mid-grey, inside every stock's plotted range.
            for step in 0..=10 {
                let log_e = 0.18f32.log10() + (step as f32 - 5.0) * 0.18;
                for ch in 0..3 {
                    let density = forward(ch, log_e);
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
        let forward = |ch: usize| -> f32 {
            let t = sc.channels[ch];
            let i = t.partition_point(|p| p.0 <= log_e).clamp(1, t.len() - 1);
            let ((x0, d0), (x1, d1)) = (t[i - 1], t[i]);
            d0 + (log_e - x0) * (d1 - d0) / (x1 - x0)
        };
        let (dr, db) = (forward(0), forward(2));
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
                let t = sc.channels[ch];
                let at = |x: f32| {
                    let i = t.partition_point(|p| p.0 <= x).clamp(1, t.len() - 1);
                    let ((x0, d0), (x1, d1)) = (t[i - 1], t[i]);
                    d0 + (x - x0) * (d1 - d0) / (x1 - x0)
                };
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
}
