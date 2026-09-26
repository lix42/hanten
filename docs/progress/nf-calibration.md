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

The epic was created on 2026-09-19 with the new-flow migration plan
(`docs/nf-migration.md`). **`scale-ladder` is done** (2026-09-20).

**`anchor-comparison` is done (2026-09-25): the roll's white is content-referenced,
placed through contrast, never through the decode's anchor.** `d` stays 0.62 and mid-grey
stays pinned. Rule, provisional values from nine rolls: each frame's white is its red p97;
the roll's white is its brightest frame's white at or under **+2.0** scene stops above
mid-grey, raised to at least **+1.5**; a frame above +2.0 is clamped to it; the solved
`look.contrast` spans whole contrast 2.23–2.97 (2.23 on a clamped frame). A warning fires near the **leader**
(film saturation), never merely because the cap bound. It was chosen by review **with a
black point in the chain**, and the new chain has none: every placement looked pale
without one. Black is now `nf-display-stages/parametric-operator`'s (reference: where the
film base renders). Implementation is `roll-white-rule`; the warning's margin is
`saturation-margin`. Ranked rule > fixed anchor > content white with solved contrast
(noise up to 5.4× the scan's floor) > level move (mid-grey up to L\* 82). Levels here
are **scene stops**: red density through the fixed linearization, 1 stop ≈ 0.167
density.

## anchor-comparison

**Status:** done
**Updated:** 2026-09-25

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
- 2026-09-24: from `nf-look/contrast` (user). **`W` is held at red p97 for the
  renders**; the final percentile, and whether it is a code constant or a recipe value,
  stays this task's to settle or hand on explicitly. **Where C/D's per-roll contrast
  lands is settled**: it is `look.contrast` itself, one knob, with the decode's anchor
  unchanged — only how it reaches the recipe (`measure-roll` or by hand) stays open.
- 2026-09-25: **started; two review rounds, and the shortlist changed shape.** Sets and
  scripts in `../temp/anchor-cap/` (`review.json` = cap round, `review-probe.json` = black
  probe); rendered by hand on the new chain, since the generator cannot pass `--new-flow`.
  Only `look.contrast` varies within a roll (total gamma `= 0.745 / (W − d)`, mid-grey
  pinned at `d = 0.62`); white balance is each roll's `measure-roll` gains, held.

  **Units.** The user reasons in stops, so every level here is in **scene stops above
  mid-grey**: red density through the decode's fixed linearization (1 stop = 0.167
  density). Not rendered stops — those depend on the contrast being solved.

  **Two measurement traps, both hit.** (1) `decode-side.json`'s per-frame `q` is the
  *channel mean*; the spike's `W` is *red*. Mixing them made frames appear to exceed their
  leader. (2) `estimate`'s film base is a **transmission**: density is
  `−log10(T / base)`, not `−log10 T − base`. Re-measured on red, all nine negative rolls
  (`remeasure-fixed.json`): no frame's white exceeds its leader, and the spike's `W`
  reproduces. The spike's roll `W` was the **p90 over frames** of per-frame red p97 — a
  cross-frame percentile the user rejected, since it makes the roll's white depend on
  roll size.

  **Replaced D with a clamp on the roll's white** (user). The roll's white is its
  brightest frame's white (per-frame red p97, which keeps the headroom), bounded by a cap
  above and a floor below. Frames whose white sits above the cap are handled one of two
  ways: **V1** clamps the roll's white to the cap; **V2** skips them and takes the
  brightest frame left (the cap if none is). A cap binding must **warn** (user).

  **Cap round** (6 rolls, 35 frames): uncapped, V1/V2 at +2.5/+3.0, V1 at leader −1.0.
  The user preferred V2 +2.5 on most frames, V1 +2.5 on some; uncapped and +3.0 were
  flat. Uncapped white lands at +2.2 to +4.7, which is gamma 0.94–2.05, pale on every roll.
  1121 (07-23) judged overexposed; 1868 and 1902 (09-20) bright scenes; 1151 ambiguous.

  **The pale look was mostly the missing black point.** The new chain places no black
  (`--black-point` is refused under `--new-flow`; `flare-removal` and the operator's toe
  are unbuilt), so the only thing darkening shadows was contrast: the darkest 1% of
  pixels sat at L\* 12–26. Every roll's darkest frame bottoms out at the **film base**
  (red p0.5 at −3.6 to −3.8 stops, against mid-grey's 3.7 above base), which the straight
  line renders at **L\* 12–15** under V1. The **black probe** moved that level to L\* ≈ 2
  (one value per roll and arm, applied to the JPEG, not a pipeline stage). Verdict (user):
  **with black added, both white rules improved** (V1+black > V1, V2+black > V2), and on
  09-14 and 09-18 the earlier wish for more contrast was met by the black alone. The
  **two-point pin** (base → L\* 2, `W` → white, mid-grey floating; gamma 2.48–2.67 on every
  roll) put mid-grey at L\* ≈ 36 and was **too dark** — **mid-grey stays pinned**.
  **Leader − 0.5 with skip** found the overexposed frames (skipped 1121, 1151, 1816) but left
  09-20 flat — it neither places the white nor fixes flatness.

  **Where V1 beat V2 (both with black): almost exactly the frames V2 skipped** — 1121,
  1151, 1632 (skin), 1868 (a toss-up); 1902, barely over the cap, went the other way. The
  one other V1 win, 1137, is on 07-24 (3 frames), where skipping 1151 makes V2 jump to gamma
  3.39: the jump to the next frame on a short roll. Hence **V3** (user-agreed): the roll's
  contrast from V2, and each skipped frame clamped to the cap as in V1, with a warning.
  Only warned frames render differently from their roll.

  **What the leader is still good for.** Its distance from a frame's white says whether the
  frame nears film saturation (1121 0.13 stop under, 1151 0.40) or is a bright scene
  within latitude (1868 1.81 under). That separates the warning's two meanings, and
  may justify a different highlight treatment for the saturated kind.

  **Consequences.** (a) A black point is needed, and it is not this task's to build:
  `nf-display-stages/parametric-operator` / `nf-scene-correction/flare-removal`. Its
  natural reference is where the film base renders, which is already measured. Until it
  exists, **every white rule must be judged with the probe's black added**, or the white
  is tuned to compensate for the missing black. (b) The cap value is still an estimate (+2.5
  was the only value tested with black); the floor is untested. Next round: V2 vs V3 at
  +2.0/+2.5/+3.0 with black, and single dark frames under a floor ladder.
- 2026-09-25: **round 3 (all with the probe's black) — cap, V3 and floor chosen** (user).
  Sets `review-r3-cap.json`, `review-r3-floor.json`; `scripts/round3.py`.

  **Cap +2.0, V3.** +3.0 lost wherever it differed; +2.0 and +2.5 were close, with a lean to
  +2.0 (09-18, 09-14). V3 was safer on the frames near film saturation (1121: "v3 is much
  safer at highlight"; 1151), V2 better on skipped frames that are ordinary bright scenes
  (1632, 1902) — the split the leader distance predicts. **The warning keys on the leader,
  not the cap** (user): +2.0 binds on every roll measured, so a cap warning would fire on
  every roll. Warn when a frame's white is within a margin of its leader ("near film
  saturation"); the cap itself is silent.

  **Floor +1.5.** Single dark frames treated as a roll of one (09-13 1698/1696, 09-11
  1658/1662, 09-09 1612, 09-14 1719, 09-18 1797, 09-20 1904): floor +1.0 (gamma 4.45) was
  worst on every frame except control 1810; +1.5 (2.97) beat +2.0 (2.23) and the frame's
  render within its own roll.

  **The rule this gives, on all nine rolls** — roll white = brightest frame white at or
  under +2.0, raised to at least +1.5; frames above +2.0 clamped to it:

  | roll | `W` | gamma | leader − frame < 0.5 |
  |---|---|---|---|
  | 07-15 Ektar100 | +1.95 | 2.29 | — |
  | 07-23 Portra160 | +1.91 | 2.33 | 1121 (0.14) |
  | 07-24 Gold200 | +1.50 floor | 2.97 | 1151 (0.39) |
  | 09-09 Ektar100 | +1.87 | 2.39 | — |
  | 09-11 Portra400 | +1.52 | 2.93 | — |
  | 09-13 Portra400 | +1.65 | 2.69 | — |
  | 09-14 Ektar100 | +1.98 | 2.25 | — |
  | 09-18 Gold200 | +1.50 floor | 2.97 | 1816 (0.27) |
  | 09-20 Portra400 | +1.97 | 2.26 | — |

  Two things this table raises, open: (1) **cap − floor is 0.5 stop**, so the rule can move a
  roll's white by at most half a stop and its contrast only across 2.25–2.97. A fixed white
  near +1.75 (gamma 2.54) is within ±0.25 stop of it on every roll, so the fixed option is the
  null to beat before the per-roll measurement earns a place. (2) A 0.5-stop leader margin
  also flags **1816**, which the user never judged overexposed; Gold200's leader sits only
  ~1.5 stops above its content on both Gold rolls, so the margin may be stock-dependent.
- 2026-09-25: **round 4, the null test — the per-roll rule beats a fixed white** (user).
  `review-r4.json`, `scripts/round4.py`: 8 frames on five rolls chosen where the two differ
  most (floor rolls 09-18/07-24/09-11 steeper under the rule, cap rolls 09-14/09-20 flatter),
  the rule against one fixed white at +1.75 (gamma 2.54), both with the probe's black. "They
  look different… maybe 1 or 2 frames the fixed one is better, but the rule is more
  consistent." So the per-roll measurement earns its place, narrow band and all. **1816**:
  the rule's highlight is better — the V3 clamp to the cap (gamma 2.23 against 2.54) helps
  it, whether or not it is near saturation, so the leader margin (0.5 stop) stays
  provisional rather than confirmed by this frame.
- 2026-09-25: **the verification numbers, and the ranking.** `scripts/verify.py`, 160
  16-bit renders measured in memory, on every frame with marked patches on 09-18/09-14/
  09-20 plus three 09-11 frames. Noise is the three-way report's measure (q25 of
  `|I(x+1) − I(x)|` over the median, red, 1200 px central crop) as a multiple of the scan's
  own floor. C\* is the mean over the band fit's marked patches. Mid-grey is where a datasheet
  mid lands, scene-referred (analytic). A = today's default; B = the spike's `W` to white by
  exposure at gamma 2.0; C = the spike's candidate C; rule = the chosen rule; rule-bk = with
  the probe's black.

  | roll | option | gamma | noise × floor (max) | white C\* | colour C\* | mid L\* |
  |---|---|---|---|---|---|---|
  | 09-18 Gold200 | A | 2.00 | 0.88 (1.03) | 3.8 | 8.4 | 49.5 |
  | | B | 2.00 | 0.77 (0.91) | 4.3 | 10.3 | **72.1** |
  | | C | 4.15 | **1.98 (2.45)** | 7.8 | 17.3 | 49.5 |
  | | rule | 2.23–2.97 | 1.32 (1.63) | 5.5 | 11.7 | 49.5 |
  | | rule-bk | 2.23–2.97 | 1.36 (1.67) | 5.5 | 11.8 | 49.5 |
  | 09-14 Ektar100 | A | 2.00 | 0.88 (1.34) | 4.0 | 11.0 | 49.5 |
  | | B | 2.00 | 0.83 (1.27) | 4.2 | 11.6 | 58.4 |
  | | C | 2.57 | 1.15 (1.76) | 5.3 | 14.9 | 49.5 |
  | | rule | 2.23–2.25 | 1.00 (1.52) | 4.5 | 12.7 | 49.5 |
  | | rule-bk | 2.23–2.25 | 1.14 (1.88) | 4.6 | 12.9 | 49.5 |
  | 09-20 Portra400 | A | 2.00 | 0.87 (1.01) | 13.7 | 31.1 | 49.5 |
  | | B | 2.00 | 0.84 (0.98) | 14.6 | 32.5 | 54.9 |
  | | C | 2.32 | 1.00 (1.18) | 16.1 | 37.1 | 49.5 |
  | | rule | 2.26 | 0.97 (1.15) | 15.6 | 36.0 | 49.5 |
  | | rule-bk | 2.26 | 1.14 (1.32) | 15.9 | 36.5 | 49.5 |
  | 09-11 Portra400 | A | 2.00 | 1.07 (1.28) | — | — | 49.5 |
  | | B | 2.00 | 0.90 (1.08) | — | — | **81.6** |
  | | C | 6.61 | **5.43 (6.31)** | — | — | 49.5 |
  | | rule | 2.93 | 1.75 (2.14) | — | — | 49.5 |
  | | rule-bk | 2.93 | 1.81 (2.22) | — | — | 49.5 |

  Readings. **Noise tracks the slope**, as the three-way report found: C costs 2× the floor
  on Gold200 and 5.4× on the underexposed 09-11. The rule's floor holds 09-11 to 1.75×. The
  black probe adds ~0.05–0.15×, most of it the measure's own normalisation (the black
  lowers the median it divides by). **B's mid-grey floats to L\* 55–82**: the washed-out
  midtone the spike predicted, worst on the underexposed roll. **Marked whites' C\* rises
  with contrast, in proportion to colour C\***. A → C on 09-18 doubles both (3.8 → 7.8,
  8.4 → 17.3, gamma ×2.07). Contrast is a per-channel power, so it multiplies residual cast
  with saturation, and highlight desaturation at its provisional band does not take it back.
  The rule's whites carry 1.1–1.45× A's chroma. 09-20's two white patches sit at C\* ~14–16
  under every option, so they are not neutral surfaces.

  **Ranking: rule (with a black point) > A > C > B.** The rule won every review round it
  was in. A is roll-consistent but pale and leaves the highlight operator nearly inert. C
  pins both ends but pays in noise, unboundedly on an underexposed roll. B loses mid-grey.
  "A, and move `d`" was not needed as the verdict: the rule's band (gamma 2.25–2.97) is what
  moving away from A bought, and the fixed +1.75 white lost to it in review.

  **The task's open questions, answered or handed on.** *Which percentile defines `W`*: each
  frame's red p97 over a 12% inset, with the roll taking its brightest frame under the cap —
  no cross-frame percentile. Moving onto `measure-roll`'s ACEScg pooling and re-checking
  the cap and floor there goes to the follow-up that implements it. *Code constant or recipe
  value, and how the contrast reaches the recipe*: handed to that follow-up, which has
  `measure-roll` compute it and the recipe carry the value, as white balance does. *Whether
  an underexposed roll is lifted*: only as far as the floor (+1.5, gamma ≤ 2.97); below it the
  roll renders dark, which is the user's call ("if a whole roll is underexposed, we should
  render them as underexposed").
- 2026-09-25: **done.** Follow-ups filed (user-approved): `nf-calibration/roll-white-rule`
  (implement the rule in `measure-roll`; depends on this task and on
  `nf-display-stages/parametric-operator`, which now also places black) and
  `nf-calibration/saturation-margin` (the leader margin, and frames near saturation).
  Cross-references were appended to `path-to-white` (band re-fit), `scene-range-mapping`
  (the floor as a noise budget), `parametric-operator` and `flare-removal`. The claims that this
  task would decide the anchor were updated where they are current guidance: guide,
  spec, open tasks, the `nf-look`, `nf-reconstruction`, `nf-retire` and `nf-calibration`
  epic summaries, `types.rs`, `look.rs`, `fixed.rs`. The band re-fit is owned by
  `nf-look/desaturation-band-refit`; a frame clamped to the cap is disclosed in the report,
  not warned about (both from a code review of this close-out). What a dependent
  needs: **`d` and the anchor rule do not move**; the per-roll value is `look.contrast`;
  judge anything that places white with a black point in the chain.

## roll-white-rule

**Status:** not started
**Updated:** 2026-09-25

- 2026-09-25: filed from `anchor-comparison`. Goal: `measure-roll` places the roll's white
  by the rule that task chose by review.

## saturation-margin

**Status:** not started
**Updated:** 2026-09-25

- 2026-09-25: filed from `anchor-comparison`. Goal: the leader margin the saturation
  warning keys on, and whether frames near saturation need their own treatment.

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
