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
- 2026-09-19: **first step run — the green split is real, and on one roll green
  alone cannot fix it.** Six cells (2 frames x green 0.77/0.84/0.90, blue held at
  0.73, exponential decode) in **17.4 s** total, ~2.9 s per cell including
  measurement — the all-cores work (#125) makes a ladder cheap enough to iterate.
  Set: `../temp/scale-ladder-probe/`, scripts beside it.

  Whole-frame grey-world means were useless as expected: they put the null at
  ~0.80 (September) vs ~0.85 (July) by one statistic and ~0.845 vs ~0.86 by
  another — the statistic chose the answer, which is Part 3's identifiability
  problem, not a measurement bug. The marked neutral patches
  (`../temp/neutral-patches/patches.json`) cover both frames, so the numbers below
  are read off genuine neutral surfaces instead.

  | patch | roll | light | a* null | b* null | gap |
  |---|---|---|---|---|---|
  | `ektar0715-971` cloud | July | shade | 0.866 | 0.873 | 0.007 |
  | `ektar0909-1608` white flag | Sept | sun | 0.811 | 0.854 | 0.043 |
  | `ektar0909-1608` cloud | Sept | sun | 0.792 | 0.877 | 0.086 |

  Two findings. **(1) The scan-date split reproduces** — July nulls at 0.866,
  September at 0.79-0.81, and the shipped 0.84 fits neither: it leaves a* +7.2
  (red) on the July patch and -7.0 / -11.0 (green) on the September ones. Opposite
  directions by roll, which is what one global value costs today. **(2) The
  September roll cannot be made neutral by green at all** — its a* and b* nulls sit
  0.043 and 0.086 apart, so no green value zeroes both, while the July patch's nulls
  agree to 0.007. That points at blue (or the green/blue relationship) differing on
  that roll too, consistent with the developer change the split is attributed to.
  `src/types.rs:698`'s "blue is the solid half" may hold per roll and still not hold
  across rolls.

  Caveats: three patches on two frames, and they are drawn from the same 31 that set
  the shipped value — so this is a re-measurement through the exponential render
  path, not independent data. The July patch is the only one in **shade** and its
  `g=0.77` cell clipped (the null is interpolated between two unclipped cells, so it
  stands). The September patches erring **green** at the shipped value is the
  direction the reviewer reports not seeing.

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
