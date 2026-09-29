# Validate reference frames by tiling

## Goal

Give the effective-area measurement (`film_base::measure_area`) a **coarse tiling**
computed in the same pass, reporting *within-tile* and *between-tile* variation
separately — so "uniformly grainy" and "sloped across the frame" stop looking alike.

This is diagnostics only: it changes warnings and the report, **not** the
estimate, so it owes no `pipeline_version` bump.

**Scope moved (2026-09-28).** `film-base/holder-masked-measurement` retired `--grid`
outright and replaced it with the effective-area median, whose only uniformity check
is one pooled `(p90 - p10) / p50` spread gated at 0.5 — a coarse "is this a
picture?" guard that a leader and a near-blank frame pass. This task is what turns it
into a uniformity verdict. The `Dmax` half is gone: the leader `Dmax` retired
(`nf-retire/dmax-machinery`).

## Why the current check cannot answer the question

`--grid` compares five fixed patches by `(max − min)`, which is driven entirely by
the two extreme cells. It cannot distinguish a smooth gradient from one bad patch,
and it says nothing about how much of the spread is simply grain.

Decomposing does answer it. Measured on two leaders (2026-08-11, 4x4 tiles over
the interior):

| Roll | between-tile spread | within-tile p05–p95 | ratio |
|---|---|---|---|
| Gold 200 | **0.0081** (0.027 stops) | 0.0830 | 10 : 1 |
| Portra 160 | **0.0390** (0.13 stops) | 0.0887 | 2.3 : 1 |

Same scanner, ~5× difference in genuine spatial structure — and it independently
reproduces the baseline report's finding that Portra 160's leader is the least
uniform of the set, with blue drifting 0.048 left-to-right. A pooled percentile
cannot see this, and five patches cannot characterise it. That the leader analysis
in `reports/sigmoid-reference-baseline.md` had to be done by a bespoke script is
the evidence this belongs in `estimate`.

## What this absorbs

- **`film-base/grid-verdict-enum`'s intent**: report a self-describing verdict, not
  a bool plus an overloaded spread sentinel. `--grid` and `GridEstimate` are already
  gone.
- **The pooled spread gate** (`AREA_MAX_RELATIVE_SPREAD`) may become redundant once a
  tiled verdict exists; decide whether it stays. Its real-scan numbers: unexposed
  frames 0.06-0.29 (wider on the thinner-scanned 2026-09 rolls, and flat — centre and
  whole-area medians agree to <1%), pictures 0.87-2.26, leaders 0.23-0.45.

## Open questions

1. **Tile count.** Enough tiles to localise a defect, few enough that each keeps a
   usable sample. 4x4 was adequate to separate the two rolls above; that is not
   the same as being right.
2. **Thresholds.** 0.0081 and 0.0390 bracket "fine" and "suspect" on two rolls,
   with one scanner, over interior boxes only. Setting a number needs more rolls,
   and `film-base/dmax-anchor-reliability` already owns the leader's
   trustworthiness — coordinate rather than deciding twice.
3. **What the verdict enumerates.** At least: uniform; grain-dominated but sloped;
   localised outlier tile. Say which are actionable and which are informational.
4. **Does it warn, or can it refuse under `--strict`?** `estimate --strict` already
   exists to stop a bad base being baked into a recipe.
5. **Memory.** `measure_area` reads a fixed ~1.5 MB histogram rather than the
   pixels; a per-tile histogram multiplies that by the tile count, so either keep
   tiles few or tile a coarser statistic.

## How to Verify

- The two rolls above reproduce their between/within figures, and a threshold
  placed between them flags Portra 160 and not Gold 200.
- A synthetic frame with pure per-pixel noise and no gradient reports large
  within-tile and near-zero between-tile — the decomposition's whole point.
- A synthetic frame with a smooth ramp reports the inverse.
- A single bad tile is localised in the report, not just summed into a spread.
- Running on `film_base::effective_area`'s rectangle, frame corners raise no false
  "light leak" on a holder-mounted scan — the failure mode the old full-frame grid
  had.

## Dependencies

- [Rebuild Dmin and Dmax measurement on area x method](holder-masked-measurement.md) —
  the measurement this tiles; it retired `--grid`
- [Reuse-ready `hanten estimate` output](estimate-reuse-output.md) — carried over from the
  retired `grid-verdict-enum`

