# nc — nf-look Progress Log

Execution log for the `nf-look` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

The creative stage the old chain never had: per-channel grade, path to white, contrast, look presets, and the stock data that survives `characteristic` leaving the decode.

Nothing has landed yet: the epic was created on 2026-09-19 as part of the new-flow
migration plan (`docs/nf-migration.md`).

## stage

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: the look stage.

## per-channel-grade

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: a per-channel grade with a mid-grey pivot.

## desaturation-spike

**Status:** not started
**Updated:** 2026-09-21

- 2026-09-21: filed once the three-way measurements had settled the structural half —
  per-channel compression against a common ceiling is what converges channels, no nc
  display tone can do it, and the operator belongs pre-branch at diffuse white. What is
  left is the **form**: a per-channel curve (what film, paper and all three converters
  do — desaturates *and* shifts hue) against a hue-preserving chroma pull (what the
  design's prose describes). Nothing has compared them, and they differ most on exactly
  the content the user cares about, saturated highlights.
- 2026-09-21: **rendered and measured; both families work and the difference between
  them is smaller than the difference between strengths.** Set in `../temp/desat-spike/`
  (8 frames x 8 configs, gain maps stripped so every cell is plain SDR). Frames chosen by
  measurement, not by eye: highlight chroma above L\* 90, median across the three
  outside producers, split into **saturated** (1820, 1799, 1793, 1796 — C\* 11.8–15.2)
  and **neutral** (1789, 1811, 1810 — C\* 1.8–2.2).

  White is pinned to **each frame's own** p97 for this set. Under the base-referenced
  anchor nothing reaches the operator's range at all (`nf-reconstruction/anchor-spike`),
  and a per-frame anchor is the wrong rule but the right control here — the question is
  the operator's form, so the anchor is held in a state where both families can act.

  Matched on how much neutral-highlight chroma each removes:

  | config | neutral C\* | removed | saturated C\* | removed | hue shift |
  |---|---|---|---|---|---|
  | control | 14.8 | 0% | 19.9 | 0% | 0.0° |
  | perch-30 | 6.9 | 53% | 11.7 | **41%** | 5.0° |
  | perch-60 | 7.0 | 53% | 10.8 | 46% | 4.6° |
  | hue-35 | 7.4 | 50% | 10.7 | 46% | **4.2°** |
  | hue-06 | 3.0 | 80% | 5.4 | 73% | 5.6° |

  **(1) At matched strength the two families are close.** `perch-30` against `hue-35`:
  the per-channel curve keeps 5 points more saturated chroma for the same neutral
  cleanup, the chroma pull rotates hue 0.8° less. Both differences are small beside what
  a strength change does, so **strength is the parameter that matters and family is a
  second-order choice** — the opposite of what the task was filed expecting.

  **(2) The per-channel curve is the more selective of the two**, which was not the
  expectation. It removes 53% of neutral-highlight chroma against 41% of saturated,
  where the chroma pull is near-uniform (50% / 46%). If that holds up it argues for the
  per-channel family on the sunset question, not against it.

  **(3) The "hue-preserving" half is only approximately so.** Lerping toward `(Y, Y, Y)`
  in linear ACEScg holds luminance exactly (measured max |ΔY| = 3e-3, quantisation) but
  is **not** a constant-hue path in CIELAB — it still rotates 4.2°. A genuinely
  hue-constant operator needs a perceptual construction, which is a finding about the
  implementation rather than about the families.

  **(4) `--display-tone shoulder` drives top-end chroma to exactly 0.0** on every frame,
  with 17–29° of hue rotation. That is flattening, not shaping — it plateaus above its
  knee and the gamut map then has no room for chroma at all. Independent support for the
  design retiring it, and the reason it failed as a gamut-map separator: it does not
  isolate the gamut map's share, it destroys the highlights.

  **Not settled: the gamut map's share.** Nothing reachable by flag turns the gamut map
  off, so `perch` against `control` measures shoulder-plus-gamut-map jointly. Separating
  them needs either a throwaway patch or a measurement of how many top-end pixels are
  out of P3 before mapping. Carried forward.

  **Awaiting the user's ranking** — the measurements cannot say whether 5 points of
  saturated chroma or 0.8° of hue is the one that shows.

## path-to-white

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: highlight desaturation.
- 2026-09-19: the gating spike moved to `nf-calibration/scale-ladder` and was
  redefined. The evidence behind this task's premise — that the knee'd sigmoid's
  clean whites come from its per-channel shoulder — does not separate the shoulder
  from the anchor (0.28 vs 0.5 mid-fraction) or the display tone (`none` vs
  reinhard), which the two presets also differ in. Whether any `density.scale`
  reaches those whites now decides if this task is load-bearing or an optional look.

## contrast

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: the print-contrast knob.

## look-presets

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: re-express the `--preset` bundles.

## stock-data-home

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: a home for the film-stock data.

## scene-range-mapping

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: spike: opt-in bounded scene-range mapping.

