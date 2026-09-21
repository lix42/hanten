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
