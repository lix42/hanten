# nc — nf-calibration Progress Log

Execution log for the `nf-calibration` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

The numbers rather than the machinery: an early `scale` ladder (runs against today's binary, no colorchecker), the `scale`/`gamma` review loop, the offset question, the neutrality release gate, and what a user would run.

Nothing has landed yet: the epic was created on 2026-09-19 as part of the new-flow
migration plan (`docs/nf-migration.md`).

## scale-ladder

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: filed after the plan review as `nf-look/path-to-white-spike`. Goal: can a look-stage operator reproduce the knee'd sigmoid's whites?.
- 2026-09-19: moved here and redefined as a `density.scale` ladder. Three reasons.
  (1) `sigmoid-flat` *is* the exponential (`toe = shoulder = 0`), so Appendix E
  already compared the exponential against the knee'd render — but the two presets
  differ in four ways at once (shoulder, anchor 0.28 vs 0.5, `print_exposure`,
  `display_tone`), so nothing in it isolates the shoulder. (2) The scale is the
  higher-leverage question and was sequenced last, behind the whole chain being
  built. (3) The git tag removed the "run it while the sigmoid still exists"
  urgency, so it no longer needs to gate `nf-core/stage-skeleton`.
- 2026-09-19: **green gets measured, not judged.** The user reports they are not
  sensitive to light green, which is the axis `src/types.rs:698` documents as
  unresolved (green splits by scan date, July 0.86–0.90 vs September ~0.77; blue is
  solid at 0.68–0.78). So the eye decides red/blue and preference; green is read off
  neutral surfaces. On the one clean neutral measured — `ektar0909-1608`'s white flag
  — the knee'd render is G/R 1.024, B/R 1.027: a mild *cool/cyan* cast, not green,
  and still the most neutral of the four. But it is one patch on one frame, so the
  knee'd render is a comparison target here, not ground truth.

## scale-gamma-loop

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: tune `scale` and `gamma` by review.

## offset-question

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: does `density.offset` earn a value?.

## neutrality-gate

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: the neutrality release gate.

## user-calibration-procedure

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: what a user would actually run.
