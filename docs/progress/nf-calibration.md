# Hanten — nf-calibration Progress Log

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

## anchor-comparison

**Status:** not started
**Updated:** 2026-09-23

- 2026-09-21: filed to carry the rendered half of `nf-reconstruction/anchor-spike`,
  which costed four white placements from the scans but could not rank them: under the
  fixed anchor the three rolls measured land 0.55–1.28 stops short of white, so a
  per-channel highlight operator has nothing to act on and a render would compare four
  configurations of which one is inert. Waits on `nf-look/path-to-white`. A verdict of
  "keep the fixed anchor and move `d` instead" is a complete outcome.
- 2026-09-23: the "0.55–1.28 stops short" above is **0.55–1.73** once 09-11's calibration
  frame is excluded — see the correction in [`white-placement.md`](../spike/white-placement.md).
  09-11 alone would ask candidate C for gamma 6.61, which makes it the natural test roll
  for D's ceiling.

## scale-ladder

**Status:** done
**Updated:** 2026-09-20

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
- 2026-09-20: **widened to every patched frame — the unit is the roll, not the scan
  date, and there is no offset signature.** 14 frames x 4 green values (0.72–0.93,
  blue held) in **2m26s**, ~2.6 s a cell. Five of the 19 patched frames no longer
  have sources under `../nc-assets` (three 2026-09-09-Ektar100, two
  2026-09-11-Portra400), which cost that Ektar roll most of its sample. Set:
  `../temp/scale-ladder/`, with `analyse.py` beside it. 22 patches, 5 rolls;
  interpolation excludes cells whose median clipped.

  | roll | n | green null | within-roll spread |
  |---|---|---|---|
  | 2026-09-11-Portra400 | 8 | 0.783 | 0.030 |
  | 2026-09-09-Ektar100 | 4 | 0.839 | **0.104** |
  | 2026-07-15-Ektar100 | 1 | 0.865 | — |
  | 2026-07-23-Portra160 | 5 | 0.877 | 0.027 |
  | 2026-07-24-Gold200 | 3 | 0.886 | 0.032 |

  **(1) Per-roll nulls are tight; roll-to-roll they span 0.103.** Four of five rolls
  agree internally to <=0.032, which is what makes a per-roll correction
  well-defined. The exception is 2026-09-09-Ektar100, and its spread is not noise —
  it splits by *frame*, 1608 (sun) ~0.80 against 1627 (shade) ~0.875.

  **(2) The scan-date story is too simple.** July's three rolls cluster
  (0.865/0.877/0.886) but September's two do not (0.783 vs 0.839), and the clusters
  overlap. Stock and scan date are confounded in this set — Portra400 is both the
  lowest null and September-only — so neither can be isolated here.

  **(3) The shipped 0.840 is a well-chosen compromise.** Patch-weighted optimum is
  0.835, roll-weighted 0.850. At 0.840 the worst roll carries -0.057 in green scale;
  at 0.850, -0.067. So retuning the global value buys almost nothing — the cost is
  the spread, not the centre.

  **(4) No offset signature, which cuts against the NLP hypothesis.** If the decode
  error had an offset component, a patch's null would drift with its lightness
  consistently. It does not: the slope is +0.027 per 10 L* on Portra160 (r=+0.97),
  **-0.024 on Ektar0909** (r=-0.94), and +0.001 on Portra400 (r=+0.05) — the
  best-sampled roll, 8 patches over L* 50–80, showing essentially nothing. Opposite
  signs across rolls means the within-roll correlations are picking up scene and
  illuminant differences, not a tone-dependent decode error. On this evidence the
  error is well modelled as a pure slope, and NLP's red shadows are more likely its
  own per-frame fitting than a term missing from ours.

  **(5) The a*/b* disagreement is not clean evidence about blue.** Median gap 0.033,
  8 of 16 patches over 0.03 — but the large gaps land on cars and clouds while walls
  and cloth sit at 0.001–0.007. A surface with a real colour produces exactly this,
  so the gap may measure patch quality rather than the decode. Separating the two
  needs the ColorChecker.
- 2026-09-20: **outside evidence corroborates the shipped value; no tweak is
  available now, and the tuning pass should wait for the anchor.** Three commercial
  converters measured against the same negatives
  (`docs/reports/three-way-gold200.md`) put green at 0.83–0.91 against nc's 0.840,
  and blue between 0.44 and 0.76 — a range too wide to override our own 31-patch
  measurement, which called blue solid at 0.68–0.78 on every roll. Neither value has
  a reason to move.

  **The optimum is anchor-independent, which is why one pass suffices.** With
  `out_c = 10^(gamma·(scale_c·D_c − A))` and a *scalar* anchor `A`, a neutral patch
  needs `scale_R·D_R = scale_G·D_G = scale_B·D_B` — `A` cancels. Moving the anchor
  changes **where the error is visible**, not what the scale should be. (That holds on
  the straight part; a shoulder compresses per channel by level, so a highlight anchor
  would *mask* scale error at the top and leave it in the midtones — an argument for
  measuring the scale in the midtones, not for measuring it twice.)

  And the widened ladder already showed there is nothing to win: patch-weighted
  optimum 0.835 against the shipped 0.840, roll-weighted 0.850, while the roll-to-roll
  spread is 0.103. Retuning the centre moves the worst-case residual from 0.057 to
  0.067 — the wrong direction. So: **do not tune now.** One pass, after
  `nf-reconstruction/anchor-rule` settles and with the ColorChecker frames in hand.
- 2026-09-20: **done.** The question is answered, negatively: the split is real and
  per-roll, and the shipped `[1, 0.84, 0.73]` already sits at the optimum of what one
  global value can do. The HDR check this task listed is moot — it applied to a
  *winning* scale, and nothing moved. The ColorChecker pass is
  [neutrality-gate](../tasks/nf-calibration/neutrality-gate.md)'s, and the post-chain
  tuning is [scale-gamma-loop](../tasks/nf-calibration/scale-gamma-loop.md)'s, which
  already depends on this task and inherits both the value and the method caution.

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
