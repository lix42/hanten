# nc — nf-reconstruction Progress Log

Execution log for the `nf-reconstruction` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

The fixed, stock-agnostic decode: exponential, one anchor rule with a frozen `d`, `gamma` split into a calibration half and a look half.

Nothing has landed yet: the epic was created on 2026-09-19 as part of the new-flow
migration plan (`docs/nf-migration.md`).

## fixed-decode

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: the fixed, stock-agnostic decode.

## anchor-spike

**Status:** not started
**Updated:** 2026-09-20

- 2026-09-20: filed after the three-way converter measurements
  (`docs/reports/three-way-gold200.md`). Goal: does a diffuse-white anchor earn its
  place? Runs against today's binary — `--anchor-white-at-reference --d-max D` is the
  candidate, `--anchor-mid-offset` the control — so it can answer `anchor-rule`'s
  highlight-vs-mid question before the rule has to be chosen.
- 2026-09-21: **decode-side half run on three rolls — under the proposed anchor no roll
  reaches white, so a highlight operator would have nothing to act on.** Scripts and raw
  data in `../temp/anchor-spike/`. Bases measured per roll with `estimate --grid` (cell
  spreads 0.007–0.021, benign gradients); densities are `D' = scale·D` with the shipped
  `[1, 0.84, 0.73]`, read on **red**, where `scale = 1` and `D' = D` — the units the
  anchor is stated in.

  The anchor arithmetic is confirmed against the binary: `mid-at-base-offset(0.62)` at
  gamma 2 reports `anchor_value: 0.99236375`, matching `d + MID_GREY_OUTPUT_DECADES/2`
  to seven figures, and sitting 0.012 above the datasheets' diffuse white at 0.98.

  | roll | n | red p97 median | p97 p90 | vs anchor | renders at | stops short |
  |---|---|---|---|---|---|---|
  | 2026-09-18-Gold200 | 35 | 0.671 | 0.800 | −0.193 | 0.412 | **−1.28** |
  | 2026-09-14-Ektar100 | 32 | 0.710 | 0.910 | −0.083 | 0.683 | −0.55 |
  | 2026-09-11-Portra400 | 12 | 0.546 | 0.860 | −0.133 | 0.543 | −0.88 |

  **(1) The open-loop diagnosis is quantified.** Every roll's bright end lands 0.55–1.28
  stops below white. Nothing enters the region a per-channel operator compresses, so
  `nf-look/path-to-white` would be inert under this decode regardless of its form — the
  two spikes are coupled through this number, not only through the argument.

  **(2) One fixed `d` cannot serve the three.** To put each roll's bright end at white
  they would need `d` = 0.428 / 0.538 / 0.488 — a spread of 0.110 density, **0.73 stops**.
  So the reading is not simply "0.62 is too high": any single value leaves the rolls that
  far apart at the top, which is the trade a content-referenced anchor removes and the
  faithfulness it gives up.

  **(3) Per-frame would move far more than per-roll.** Within-roll spread of per-frame
  red p97 is 0.40 / 0.40 / 0.96 in `D'` — Portra400's widest frame sits 1.3 while its
  narrowest sits 0.33. Confirms roll granularity as the unit; a per-frame anchor would
  re-expose every frame.

  **Aside, and not this task's:** `--display-tone none` refused a Gold200 frame outright
  because the **film holder** renders above reference white (opaque → high density →
  bright positive). `effective_area` reported `holder: 0` on all four edges with
  `holder_applied: false` and only its 5% static inset (167 px of 3343), which does not
  reach the holder on this scan. Relevant to `film-base/holder-cap-contamination`, and a
  trap for any measurement taken on a full frame rather than an inset one.

## anchor-rule

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: one anchor rule, with a value for `d`.

## gamma-split

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: split `gamma` into calibration and look.

## curve-endpoint-warning

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: warn when the curve's endpoint is unreachable.

## mono-decode

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: where black-and-white fits the new chain.
