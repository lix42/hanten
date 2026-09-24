# Hanten — nf-docs Progress Log

Execution log for the `nf-docs` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

Fold the new design into the spec, the user guide and CLAUDE.md.

Created on 2026-09-19 with the new-flow migration plan (`docs/nf-migration.md`).
The only change so far is CLAUDE.md's layout (2026-09-24, `claude-md`): it now
holds only cross-cutting rules, and subsystem traps live in module docs.

## design-spec

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: fold the new design into the spec.

## using-nc

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: bring the guide up to the new flow.

## claude-md

**Status:** in progress — layout done; the architecture rewrite waits on the flip
**Updated:** 2026-09-24

- 2026-09-19: created with the new-flow plan. Goal: update claude.md for the new architecture.
- 2026-09-24: CLAUDE.md restructured (1,428 → ~300 lines) ahead of this task. It
  now holds only cross-cutting rules; each subsystem's traps were checked against,
  or moved into, the `//!` docs of its module, and tool notes into nested
  `tools/review-app/CLAUDE.md`, `scripts/analysis/CLAUDE.md` and skills. The old
  HDR-framing paragraph is gone. What remains for this task: replace the
  current-chain diagram with the new stage sequence once `nf-core/default-flip`
  lands, update the "Where the detail lives" table for retired modules, and
  retire the migration rule.

## reference-sweep

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: filed after the plan review. Goal: re-point references to retired and superseded tasks.
