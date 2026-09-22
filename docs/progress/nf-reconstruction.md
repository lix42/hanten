# Hanten — nf-reconstruction Progress Log

Execution log for the `nf-reconstruction` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

The fixed, stock-agnostic decode: exponential, one anchor rule with a frozen `d`, `gamma` split into a calibration half and a look half.

No code has landed yet — the epic was created on 2026-09-19 with the new-flow plan
(`docs/nf-migration.md`) — but **one spike is done and its result is an input to other
epics**. `anchor-spike` costed four ways to place the decode's white
([`docs/spike/white-placement.md`](../spike/white-placement.md)) and found that under
today's proposed anchor all three rolls measured land **0.55–1.28 stops short of white**,
so a per-channel highlight operator has nothing to act on. Anyone building on this epic,
and `nf-calibration/anchor-comparison` in particular, needs that before they start.

## fixed-decode

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: the fixed, stock-agnostic decode.

## anchor-spike

**Status:** done
**Updated:** 2026-09-21

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
- 2026-09-21: **done**, having costed a shortlist rather than picked from it. The
  numbers live in [`docs/spike/white-placement.md`](../spike/white-placement.md);
  the summary below is the decision trail. Four
  placements, with what each gets right on 2026-09-18-Gold200 (`W` = 0.800, `d` = 0.62):
  **A** fixed anchor pins mid and reaches 0.412; **B** content white reaches white and
  puts a datasheet mid at 0.437; **C** pins both by solving
  `gamma = MID_GREY_OUTPUT_DECADES / (W − d)` = 4.15; **D** is C with a noise ceiling,
  sliding toward B when the cap binds. C is the only one that pins both ends, and
  structurally — two free parameters against two constraints. It stays inside the design
  because `gamma` already splits, so its per-roll contrast (2.31× / 1.43× / 1.73× over
  the 1.8 linearization) is the look stage's knob and the decode stays fixed. Its cost is
  that it asks Gold200 for more contrast than any converter used on that roll.

  The rendered ranking is deliberately **not** here — under A nothing reaches the region
  a per-channel operator acts in, so a render now would compare four configurations of
  which one is inert. It moves to
  [`nf-calibration/anchor-comparison`](../tasks/nf-calibration/anchor-comparison.md),
  which waits on the operator.

## anchor-rule

**Status:** not started
**Updated:** 2026-09-22

- 2026-09-19: created with the new-flow plan. Goal: one anchor rule, with a value for `d`.
- 2026-09-22: **how a content-referenced rule would be spelled, decided before the task
  starts.** Not which rule — that is still this task's call — but where its parts live if
  it reads content. The **rule** stays a look knob and the **measurement** it consumes
  becomes a third `calibration` key beside `film_base` and `dmax`; that is the
  `AnchorPlacement` pattern already in the tree, which design-spec §8 states as "the
  anchor is the rule for what the reference places" with only the measured value leaving.
  `core/calibration-recipe-section` has been asked to keep that section open rather than
  model it as a closed pair.

  Four things that follow, now in the task file: record the **percentile with its value**
  (p95/p97/p99 differ by about a stop on one roll, so a bare scalar loses the definition)
  plus probably the per-frame spread, since telling a flat roll from a wrong `d` is
  exactly what candidate D exists for and candidate C cannot do; **nothing measures it
  today** — `estimate` reads one frame, a roll percentile means reading the roll with
  provenance and confidence, i.e. `core/base-acquisition-planner`'s cascade; it is
  **content-derived, not reference-derived**, unlike the rebate and the leader; and the
  placement **stays an enum** whatever is picked, because `nf-calibration/anchor-comparison`
  has to render the candidates to rank them.

  **What the candidates actually cost, sharpened by the `fixed-decode` session the same
  day.** The placement is a two-parameter family `(d, gamma)` and all four candidates are
  points in it, because `A = d + MID_GREY_OUTPUT_DECADES / gamma` can be solved either
  way: the hybrid fixes `d` and solves `gamma = M / (W - d)` to get `A = W`; the level
  move fixes `gamma` and solves `d = W - M/gamma`, which at gamma 2.0 on Gold200's
  `W = 0.800` is **0.4276** — and `white-placement.md`'s own table already prints
  `d = 0.428 / 0.538 / 0.488` for the three rolls. So **every candidate is reachable on
  today's binary** with `--density-curve exponential --anchor-mid-offset <d>
  --density-gamma <g>` (the curve selector is required: `merge` refuses `--density-gamma`
  beside the resolved sigmoid default). B and C were rendered at exit 0 to confirm.

  **The consequence is the point of this entry.** B/C/D do not need a decode change —
  they need a *measurement* of `W` and somewhere to put it. The anchor grows a variant
  only if it is to **consume** that measurement rather than take a hand-set number, which
  is the same distinction `film_base::estimate` draws by taking a *resolved*
  `&FilmBaseSource` rather than the params object.

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
