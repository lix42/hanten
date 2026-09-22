# Hanten — nf-look Progress Log

Execution log for the `nf-look` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

The creative stage the old chain never had: per-channel grade, path to white, contrast, look presets, and the stock data that survives `characteristic` leaving the decode.

No code has landed yet — the epic was created on 2026-09-19 with the new-flow plan
(`docs/nf-migration.md`) — but **one spike is done and it settles what `path-to-white`
ships**. `desaturation-spike`
([`docs/spike/highlight-desaturation.md`](../spike/highlight-desaturation.md)) chose a
**chroma pull** over a per-channel curve — not on appearance, which is equivalent
(ΔE ≈ 0.7), but because the curve moves luminance too and the fit range downstream eats
whatever compensates it. Its strength must key on **distance from the neutral axis as
well as brightness**, or a bright coloured surface is neutralised as hard as a bright
white one. It is a highlight operator and cannot reach cast below about L\* 70.

**`path-to-white` is built against a hand-set contrast (user decision 2026-09-22).**
Under the base-referenced anchor at contrast 2.0 the operator is **inert** — all three
measured rolls land 0.55–1.28 stops below white — so the task is developed with a
per-roll `--density-gamma` computed from that roll's base and red p97 (candidate C/D in
`docs/spike/white-placement.md`), and its band values are provisional until
`nf-reconstruction/anchor-rule` chooses the rule. Two tasks split out of it so they can
run now against today's binary: `desaturation-band-fit` here, and
`nf-display-stages/gamut-map-share`.

## stage

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: the look stage.

## per-channel-grade

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: a per-channel grade with a mid-grey pivot.

## desaturation-spike

**Status:** done
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

- 2026-09-21: **the user's verdict closed it, and corrected the experiment on the way.**
  Comparing #2 (per-channel, shoulder 0.3) against #5 (chroma pull 0.35): *"I can tell the
  bright change, but I cannot tell the color change."*

  That was a flaw in the set and a finding at once. The two arms were matched on **chroma
  removal** and left unmatched on **brightness**, because the per-channel curve compresses
  luminance as well as converging chroma while the pull holds luminance by construction:

  | config | L\* top | vs control | C\* top | removed |
  |---|---|---|---|---|
  | control | 80.6 | +0.0 | 17.0 | 0% |
  | per-channel 0.3 | 76.6 | **−4.0** | 9.0 | 47% |
  | chroma pull 0.35 | 80.6 | **+0.0** | 8.8 | 48% |

  **Matching the brightness needed `--print-exposure 0.38` and two attempts** — the first
  at 0.19 recovered only half, because the exposure is applied *before* the fit range and
  reinhard re-compresses the lift. With it matched:

  | | L\* top | C\* top | hue shift |
  |---|---|---|---|
  | per-channel, matched | 80.4 | 9.5 | 5.1° |
  | chroma pull | 80.6 | 8.8 | 4.3° |

  **ΔL\* 0.16, ΔC\* 0.66, Δhue 0.7° — a total ΔE of about 0.7, below visibility.** At
  matched brightness the two families produce the same picture, which is what the eye
  reported before the measurement caught up.

  **So the form question is closed, and not on appearance.** What separates the families
  is that the per-channel curve **entangles tone with chroma** and the pull does not —
  and the entanglement cannot be undone by a scalar, since whatever exposure compensates
  it is partly eaten by the fit range downstream. In a staged chain where fit range
  already owns luminance, an operator that also moves luminance double-compresses and
  then needs a correction that does not fully land.

  **Recommendation for [path to white](../tasks/nf-look/path-to-white.md): the chroma
  pull**, chosen for **separability, not for looks**. Its remaining open question is no
  longer "which family" but what "approaches white" is measured on, and at what strength.
- 2026-09-21: **the user marked 12 patches on 7 frames, and they found the case the
  aggregate hid.** Measured per patch rather than over the top 3%
  (`../temp/desat-spike/patch-measurements.json`).

  **(1) The operator's reach is exactly its threshold, and two marked surfaces are
  outside it.** The chroma pull starts at linear luminance 0.5, about L\* 76, and how
  much a patch moves scales with how far above that it sits — 96% at L\* 86.9, 44% at
  80.5, 25% at 74.9, and **0% at 67.9 and 63.9**. The user's "fog" (L\* 63.9, C\* 10.4)
  and one "cloud" (L\* 67.9, C\* 5.9) are untouched by **every** configuration.

  That is not a defect — it is the boundary of what a *highlight* operator can do. Cast
  on a surface at L\* 64 belongs to [the per-channel
  grade](../tasks/nf-look/per-channel-grade.md) or to the decode's `scale`, and this
  operator will never reach it. It also means **judging "are the whites clean" on a frame
  whose white sits at L\* 64 measures the decode, not the operator** — the same
  measure-the-right-thing trap as the white-patch bias in Part 3.

  **(2) The families diverge on exactly one patch, and it is the sunset case.** Matched
  on average cleanup, 11 of 12 patches agree within C\* 1.4. The twelfth is **1820 "sand
  beach"** — the brightest and most saturated of them, L\* 86.9, control C\* 27.5:

  | | C\* kept |
  |---|---|
  | per-channel, brightness-matched | **7.6** |
  | chroma pull | **1.1** |

  The pull neutralises a bright warm surface almost completely; the per-channel curve
  keeps two-thirds more of it. **This corrects the earlier "indistinguishable" reading**,
  which averaged over the top 3% where such surfaces are rare — one marked patch was
  worth more than eight frames of aggregate.

  The mechanism is that the pull's strength keys on **brightness alone**, so a bright
  *coloured* surface is neutralised as hard as a bright *white* one. That is the sunset
  problem, reproduced on a real patch, and it sharpens the open question: keying on
  distance from the neutral axis as well as on luminance would protect the sand beach
  while still cleaning the cloth. **What "approaches white" is measured on is now the
  live design question, not the family.**
- 2026-09-21: **the saturation guard works, and is strictly better than either family
  on this evidence.** Added a third and fourth parameter to the throwaway operator: a
  band over linear-RGB saturation `(max−min)/max`, full pull below `s0`, off above `s1`,
  linear between. Placed at **0.30 → 0.45** from the patches themselves — the
  whites-with-cast run 0.137–0.279 and the sand beach sits at **0.463**, 1.7× the next
  highest, so a band rather than a single knee is what separates them.

  | | whites with cast | genuinely coloured |
  |---|---|---|
  | control | 12.9 | 27.5 |
  | per-channel, brightness-matched | 5.7 | 7.6 |
  | chroma pull, unguarded | **6.1** | **1.1** |
  | **chroma pull + guard** | **6.1** | **22.3** |

  Identical cleanup on the whites (6.1 either way, and every other patch unchanged to
  0.1), and the sand beach keeps **22.3 against 1.1**. The per-channel curve's 7.6 was
  the best of the two families; the guard beats it threefold without giving up any
  cleaning.

  **So the answer to "what does approaching white get measured on" is: not luminance
  alone.** Keying strength on brightness *and* on how far the pixel already is from the
  neutral axis is what lets one operator clean a cast without flattening a sunset —
  which was the objection that made "off must stay available" feel mandatory. It may
  still be wanted, but for less.

  **Caveat, and it is not small.** The band was placed from 12 patches on one roll, and
  the sand beach is the only one above it. The *mechanism* is demonstrated; the
  *parameters* are fitted to a single example and should not be carried into
  `path-to-white` as values. What carries is the shape: two thresholds, on saturation,
  multiplying the brightness term.
- 2026-09-21: written up as [`docs/spike/highlight-desaturation.md`](../spike/highlight-desaturation.md); the entries above are the execution trail, the report is the result.
- 2026-09-21: **confirmed by eye — "I can see the diff between 3 and 4. 4 keeps the
  color."** The guard is visible, not just measurable, on 1820's sand beach at the
  strength that cleans the whites. Spike **done**; the throwaway operator is reverted and
  never merged.

  **Verdict for [path to white](../tasks/nf-look/path-to-white.md):**

  1. **A chroma pull, not a per-channel curve** — chosen for **separability**, since the
     per-channel curve moves luminance too and the fit range downstream eats whatever
     compensates it. The two are perceptually equivalent otherwise (ΔE ≈ 0.7 matched).
  2. **Strength keys on brightness *and* on distance from the neutral axis.** Brightness
     alone neutralises a bright coloured surface as hard as a bright white one. This is
     the spike's main result and it was invisible to every aggregate — one marked patch
     found it.
  3. **It is a highlight operator and cannot be more.** Its reach is its threshold:
     surfaces at L\* 64–68 are untouched at any setting, so midtone cast belongs to the
     per-channel grade or the decode. The user confirmed those read fine, consistent with
     the eye being less sensitive to cast in shade.
  4. **"Off" is still wanted but for less** than the design assumed — the guard removes
     the flatten-a-sunset objection rather than the hide-a-cast one.

## path-to-white

**Status:** not started
**Updated:** 2026-09-22

- 2026-09-19: created with the new-flow plan. Goal: highlight desaturation.
- 2026-09-19: the gating spike moved to `nf-calibration/scale-ladder` and was
  redefined. The evidence behind this task's premise — that the knee'd sigmoid's
  clean whites come from its per-channel shoulder — does not separate the shoulder
  from the anchor (0.28 vs 0.5 mid-fraction) or the display tone (`none` vs
  reinhard), which the two presets also differ in. Whether any `density.scale`
  reaches those whites now decides if this task is load-bearing or an optional look.
- 2026-09-22: **decided how this task gets built, before it can start.** The anchor
  spike's number changes the premise: under the base-referenced `mid-at-base-offset`
  anchor at contrast 2.0, all three measured rolls land **0.55–1.28 stops below white**
  ([`docs/spike/white-placement.md`](../spike/white-placement.md)), so this operator would
  be inert in a shipped render whatever form it takes. The lift comes from letting the
  roll's own content drive **contrast** rather than moving a level — candidates **C**
  (pin mid and solve `gamma = MID_GREY_OUTPUT_DECADES / (W - d)`; 4.15 / 2.57 / 3.11 on
  the three rolls) and **D** (C with a gamma ceiling, so the noise budget is explicit
  rather than accidental).

  That rule belongs to `nf-reconstruction/anchor-rule`, which is expected to land
  **after** this task. **User decision (option a):** build and verify this task against a
  **hand-set per-roll contrast** — flags only, no new code — and treat the band's values
  as provisional, re-fitted once the anchor rule is chosen. The control is
  `--density-curve exponential --anchor-mid-offset <d> --density-gamma <g>` with `g`
  computed per roll from that roll's measured base and its red p97.

  Two things this changes about the tuning, both recorded on the task file:

  **(1) Do not carry the spike's band values over.** The spike pinned white per frame at
  p97, which is a *level* move; a contrast move steepens everything below white too, so a
  different population of pixels lands in the operator's range. Re-placing the band under
  the wrong white is the same work twice.

  **(2) The threshold must become scene-referred.** The throwaway operator started at
  *rendered* linear luminance 0.5 (~L\* 76), i.e. after fit range, while the design
  requires the trigger at diffuse white, pre-branch. Once the decode pins mid — and on a
  straight line a mid anchor and a white anchor are one rule — diffuse white is a known
  value at the decode's output, so the threshold is expressed against that. The
  operator's anchor *is* the decode's anchor.

  Also noted: `nf-calibration/anchor-comparison` depends on this task while this task
  needs an anchor that reaches white. The task graph stays acyclic and nothing records
  that second direction as an edge — the coupling is the hand-set contrast.
- 2026-09-22: two pieces split out so they can run now, against today's binary, instead
  of waiting three tasks for the look stage: `nf-look/desaturation-band-fit` (the band's
  numbers, fitted to one patch on one roll) and `nf-display-stages/gamut-map-share` (how
  much of the convergence is the gamut map already). Both are now dependencies.

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

## desaturation-band-fit

**Status:** not started
**Updated:** 2026-09-22

- 2026-09-22: filed. Goal: place the saturation band from a distribution of marked
  patches rather than from the single "sand beach" example on `2026-09-18-Gold200`. The
  spike itself says the shape carries and the parameters do not, and names more marked
  saturated patches on more rolls as the cheapest way to advance it. Runs against today's
  binary — throwaway operator, anchor set by flag, patches marked in the review app — so
  it does not wait for the look stage. Measure under the hand-set candidate C/D contrast
  `path-to-white` will use, not the spike's per-frame p97 white.
