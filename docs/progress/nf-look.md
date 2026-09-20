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

