# Design update

**Status:** the design this document set out — the fixed decode (Part 1) and the staged
rendering chain (Part 2), agreed 2026-09-16/17 and extended by dated sections until
2026-09-28 — shipped with the `nf-*` migration and **now lives in
[`design-spec.md`](design-spec.md)**, the sole maintained design source
(`nf-docs/design-spec`, 2026-09-30). What stays here is what the spec does not carry:
how the decode is evaluated and tuned (Part 3), the placeholder for the same question
about rendering (Part 4), and the evidence (the appendices, including Appendix F's
record of superseded claims). Git history keeps Parts 1 and 2 as written.

# Parts 1 and 2: where each section went

Many documents cite this file by part and heading. Each heading now lives here:

| Part and heading | Now |
|---|---|
| 1, "Why this was needed" | history; git |
| 1, "The goal" | design-spec §7 (the quote) |
| 1, "The model: a traditional colour print", "Why not per-stock inversion as the default", "What reconstruction does not decide", "Known limits" | design-spec §7.1 |
| 1, "Key argument: pick reconstruction by what its output means" | design-spec §7.1 |
| 1, "The decode's parameters, and which of them are really rendering", "Decisions", "Knobs currently in reconstruction, sorted" | design-spec §7.2 |
| 1, "Methods under this goal", "The sigmoid shoulder is a contract violation" | design-spec §7.3 (retired); the exponential-vs-`generic-c41` question, §13 |
| 1, "Calibration: the part that is a measurement", "Why dividing by the base is not enough" | design-spec §7.4; the numbers, Appendices B and C |
| 1, "What NLP's white-balance step does, and why ours differs" | Appendix D |
| 1, "Colour: what 'character' is", "Kinds of character", "Dye layers and the NC film RGB v1 3×3" | design-spec §7.5 |
| 1, "Handed to rendering" | design-spec §6, "The rendering stages" |
| 1, "Removal constraints", "Reference for the migration" | done (`nf-retire/*`); the reference build is `scripts/reference-snapshot/` |
| 1 and 2, "Open questions" | design-spec §13 |
| 2, "The stages", "Decisions" | design-spec §6; the retired flags, §9 |
| 2, "Three controls the look stage owes", "A per-channel grade with a pivot", "Highlight desaturation (path to white)" | design-spec §6, "The rendering stages"; the knobs, §9 Look |
| 2, "The shadow end: reinhard compresses upward only" | design-spec §6, "The rendering stages"; §9 Fit range; the slope measurements, `pipeline::fit_range`'s docs |
| 2, "A 'direct' preset for external editing", "Two renderings: `direct` and `default`", "Keeping `direct` current as stages change" | design-spec §6, "Two renderings" |
| 2, "One contrast knob, three quantities" | design-spec §9 Look |
| 2, "Recipe warnings, not refusals" | design-spec §8 |
| 2, "`film-master` is the reconstruction output" | design-spec §6, "The film master is the reconstruction output" |

# Part 3: Evaluating and tuning the decode

Because the decode is invertible, "how much information survived" cannot grade
it. The final image can — but only with **rendering held fixed**, which is what
makes a review set evidence about the decode rather than about a redesigned
rendering. What is left to judge is the two knobs that are the decode's own
rather than duplicates of rendering: `scale` (a cast that grows with brightness)
and `gamma`'s calibration half. Both must be right before the 3×3, where a
rendering grade cannot reach them. Everything else — exposure, white balance,
the anchor — is a convention here and a control there.

**And the rendering that is held fixed shapes what the eye can see.** Measured
2026-09-17: a per-channel highlight compression desaturates whites toward
neutral, so judging cast on white surfaces structurally favours any config that
has one. With a luminance-preserving operator the same decode shows its cast.
Compare decodes under one operator, and prefer midtone neutrals for accuracy.

## What is measurable, and what is opinion

| Question | How it settles |
|---|---|
| `scale` vs `offset` | **Measurable.** A neutral surface must give `D′_r = D′_g = D′_b`. Across several densities, the residual's **slope** is `scale` and its **level** is the offset (equivalently the base). |
| `gamma`'s linearization half | **Measurable, but only with a bracket:** if exposure doubles, the reconstructed value must double. That measures the film's own slope. |
| `gamma`'s print-contrast half | **Opinion.** No neutral reference constrains it, and it belongs to rendering anyway. |
| Whether a per-channel gain suffices | **Measurable:** whatever is left after the best `scale` is the evidence for how much of a 3×3 is needed. |
| Model stability | **Needs a reference after all.** Fitting `scale` per frame and comparing the spread looks reference-free, but a per-frame fit absorbs scene colour and illuminant, so the spread partly measures subject matter — and a model that suppresses real between-frame differences scores *better*. Usable only on known-neutral or bracketed captures under one illuminant. |

## The limits of the data we have

- **The bracketed grey card is postponed, deliberately.** It needs a card
  bought, frames shot on several stocks, developed and scanned.
  `analysis/calibration-frame-capture` holds the protocol and is a **release
  gate, not a blocker**: work continues on visual review until the frames exist.
- **The 31 marked patches are white surfaces, and only approximately neutral.**
  In ordinary photographs the only findable neutral is white; the eye cannot
  tell a slightly tinted grey from a pure one, and cloud and snow carry the
  sky's blue.
- **Their density range is real but confounded with illumination**, so they can
  anchor a level, not a slope — and the 2026-09-17 offset test is what that
  confound predicted: neither offset candidate improved the render. (Appendix
  C.)

## The interim method: compare against other converters

Four producers on the same frames: **NLP**, **SilverFast with CCR**,
**SilverFast without CCR**, and nc's tuned `sigmoid-knees` as the in-house
reference. It is not ground truth, but it makes nc comparable to its
competitors, and three independent converters disagreeing with nc *in the same
direction* is evidence where one disagreeing is not.

- **What the SilverFast pair actually is.** CCR off still applies a per-stock
  NegaFix profile, i.e. the *per-stock inversion the design demotes* (design-spec §7.1) — so agreement
  with it is evidence for that design, not for ours, and run across stocks it is
  the sharpest available test of the fixed decode's central claim. CCR on adds per-frame
  cast removal, so their difference is **SilverFast's estimate of the cast**,
  not a measurement of it: it embeds the same grey-world prior as NLP's
  per-frame fit, and it contains the real scene illuminant, which is exactly
  what must be kept separate. Useful as a reference, not as a measurement.
- **Do not chase adaptation.** NLP and CCR-on both neutralize per frame, which
  nc rejects by principle (and which is why NLP collapses on a frame filled by
  one surface). The useful reading: where the references agree with each other,
  treat it as evidence about the scene; where they differ from nc **the same way
  on every frame**, that is a fixed error in the decode, i.e. `scale` /
  `offset`; where they differ **per frame in different directions**, that is
  their adaptation. A table across many frames separates those; the eye on one
  frame cannot.
- **A consensus reference is cheap, for the slope only.** The references are
  images, so `nctool metrics` reads them. But NLP and CCR-on share a grey-world
  prior, so averaging them shrinks the apparent spread without cancelling the
  bias: two families, not three votes. That shared bias is approximately a
  per-frame per-channel **gain**, i.e. a level, so a consensus is defensible for
  the **slope** and not for the level — which is the half we most need.
- **Estimate the slope within a frame, never by pooling frames.** Every
  reference except CCR-off re-balances per frame, and a per-frame gain is a
  per-frame density offset, so pooling patches across frames confounds the slope
  with each frame's own offset. One surface at two densities *in the same frame*
  is the measurement that identifies it.

### What to hold fixed, or the comparison means nothing

- Match brightness before judging colour (the +1 stop review showed preferences
  move with it).
- One nc rendering config across every nc variant in the set.
- Same output space, same viewer.
- **Judge colour more than tone.** NLP and SilverFast bake their own looks, so a
  contrast comparison mostly compares looks, while a cast comparison transfers.

## Tuning order

The loop tunes the **decode's** `scale` while rendering is held
fixed, and `--rendering direct --range sdr` (design-spec §6, "Two renderings") is the
rendering to hold:
with the roll section unapplied, white balance identity and the look reduced to the
pinned contrast, what the eye judges is the decode. The
2026-09-17 caution still applies in that setup: a defect seen there may belong
to the fixed rendering rather than to the decode, so a candidate that loses
should be re-checked under a second rendering before the decode is blamed.

The loop settles the **common-ground** values, not per-frame neutrality: one
`scale` cannot fit every scan, and the residual is rendering's to grade. `scale`
first, then `gamma`: the cast is the open question, and contrast is easier to
judge once the cast is settled. Two or three candidates per review set
keeps a frame's toggle manageable. `#124`, the 2026-09-17 offset test and the
2026-09-27 blue round (`nf-calibration/scale-gamma-loop`: nothing moved; `scale` splits
by roll) are the rounds so far. The linearization is not judged this way: on a neutral it
acts only through its product with the look's slope. The next pass is against the
calibration frames (`nf-calibration/neutrality-gate`).

**Note what the offset test showed about the loop itself.** The candidates were
built from patch arithmetic and the arithmetic preferred them; the eye rejected
both. The patches cannot see a per-frame illuminant, and they cannot see what
the display operator does to a white. So a candidate that wins on patches still
has to win on the picture, and when it does not, the disagreement is information
about the *measurement*, not only about the candidate.

# Part 4: Evaluating and tuning the rendering — not yet planned

Deliberately empty. The same questions (what is measurable, what is opinion,
what to compare against) apply to the rendering stages, but they cannot be
answered before the decode is settled: every rendering judgement made on top of
a moving decode has to be redone. Recorded here so the gap is visible rather
than forgotten.

# Appendices

The design (`design-spec.md`) states what a decision rests on; these carry the
measurements behind it. The permanent record is `docs/progress/algo.md` and `docs/progress/io.md` —
where an appendix and a log disagree, the log wins.

## Appendix A — The datasheet and the film base

- **The flat left end of a curve is base + fog, i.e. Dmin**: film that got no
  image exposure, matching the unexposed rebate in a scan. nc's tables store
  density above Dmin.
- **B > G > R density is the orange mask.** Density measures how much light is
  blocked; blocking blue most and red least looks orange. Scan and sheet agree
  in order: an Ektar scan base of `[0.53, 0.26, 0.16]` transmission (design-spec
  §4) is D `[0.28, 0.59, 0.80]`, against the sheet's Dmin `[0.21, 0.63, 0.84]`.
  Measured bases move with development — this roll's own values live in the
  review notes — so only the ordering transfers.
- **The x-axis position encodes film speed only** (absolute lux-seconds). nc
  discards it and places each stock by its published grey-card aim density.
- **Curves are measured on a neutral exposure under the rated light**, so all
  three layers received the same exposure at every point.

## Appendix B — The slope the base division leaves, and the gap to the sheets

Dividing the scan by the film base fixes the **offset**, not the **slope**. It
makes unexposed film neutral (`D = 0` in every channel), which removes the
mask's constant part. What remains is that each channel's density **rises at a
different rate** with exposure: an error that is zero where you normalized and
grows with density, so it is worst in the highlights. Contributors: the film's
own layer gammas differ (the registry's corpus rule is blue 12–19 % steeper than
red, green 2–5 %; they look parallel on the page only because the plot is
logarithmic); interimage effects, which are cross-channel and per stock; the
scanner's channels each integrate a band overlapping more than one dye, so what
they report is not the dye's own density, and that cross term depends on the dye
set; and development, which splits green by date on one scanner.

**How far our fit is from the sheets, stated carefully.** The sheets'
per-channel structure was fitted as a **(gain, offset) pair** — generic green
0.977/−0.036, blue 0.860/−0.057 — while our `[1, 0.84, 0.73]` is a gain alone,
so comparing the two numbers directly is not like for like. Collapsed onto a
zero-offset gain at our own patch densities the sheets imply ≈0.94 green and
≈0.80 blue, so the real disagreement is ≈10 %, not the factor of two an earlier
draft claimed. Blue is close to the sheets once the offset is respected; **green
is the term neither the sheets nor a per-channel model explain**, and closing it
is `io/scanner-density-calibration`.

`docs/progress/algo.md` (2026-09-04, 2026-09-06) carries the per-stock tables
and the drift measurements.

## Appendix C — The marked patches: what they are, and what they can measure

31 hand-marked patches over five rolls, in `../temp/neutral-patches/`
(uncommitted — the frames are the user's photographs). They are the data behind
`density.scale = [1, 0.84, 0.73]` (`pipeline_version` 5) and behind the
2026-09-17 offset test.

- **What they are:** 29 `white` and 2 `grey` — cloud x10, walls, cloth, cars,
  snow, a flag, a boat, a lily, a door, a curtain. No grey cards. White is the
  only neutral findable in ordinary photographs, which is the limitation, not a
  choice.
- **Density range is real but confounded.** Red density spans 0.28–1.33, and
  sorted by density the patches sort by *illumination*: the dark end is shade,
  interior, sunrise and sunset, the bright end is sun. Shade is bluer, so its
  blue layer genuinely received more exposure.
- **Which is why a two-parameter fit is unstable.** Per roll, adding an offset
  cuts the blue residual sharply (Ektar 09-09 rms 0.087 → 0.020; Portra 160
  0.081 → 0.052) — the term is real — but the fitted values scatter far beyond
  anything physical: blue gains 0.63–1.18 with offsets −0.50…+0.07, against the
  datasheets' −0.002…−0.101. Pooled over all 31 the fit is green 0.902/−0.073,
  blue 0.790/−0.075.
- **The development split.** Green wants ≈0.86–0.90 on the July rolls and ≈0.77
  on the September ones, on the same scanner and with the same stock; blue wants
  0.68–0.78 everywhere. The July rolls were developed in CineStill by the user's
  recollection, and the unexposed frames agree something changed (the September
  base is ~0.15–0.17 density denser). Recorded in `docs/progress/io.md`,
  2026-09-16.

## Appendix D — What NLP appears to do

Negative Lab Pro's workflow starts by white-balancing on the film base in
Lightroom. That is **the same operation** as nc's base division, in another
domain: a per-channel gain on the negative is a per-channel offset in density.
(Not exactly — Lightroom's white balance runs through the camera profile and its
matrix — but close.) Crucially it also fixes only the constant and leaves the
slope.

NLP's next step **appears** to fit each channel's range onto the output range
per frame, which would be a per-channel slope + offset derived from the frame's
own content. Treat that as a hypothesis, not a finding:
`algo/contrast-latitude-spike` records that the measured spreads are *compatible
with* the model rather than evidence for it, and that the single-surface
collapse (`ektar0909-1612`) is an observation to explain — a monotone stretch
destroys nothing by itself, so the mechanism that loses the detail is
unidentified. Whatever it is, nc's roll-consistency principle rules out fitting
per frame, so nc's equivalent is the same two numbers **measured per roll** and
frozen into the recipe: not "datasheet vs content", but "fitted per frame vs
measured per roll".

## Appendix E — The 2026-09-17 offset test

Set: `../temp/offset-test/`, six frames x four configs, reviewed with the gain
map stripped so every cell is plain SDR. Configs: `sigmoid-flat` with the
shipped gain `[1, 0.84, 0.73]`; the same with the datasheet-fitted pair (`[1,
0.977, 0.860]` + `[0, −0.036, −0.057]`); the same pair re-fitted to our own
patches (`[1, 0.902, 0.790]` + `[0, −0.073, −0.075]`); and `sigmoid-knees`.

**What the test rejected.** Two *values* for `offset`, not the term: the
datasheet-fitted pair and one re-fitted to our own patches. The term stays open
(design-spec §13); what is missing is data that can identify a value.

**Why the offset was a candidate.** It is physically distinct from the film base
even though the two share an axis: the base is *measured* from the rebate, so
the offset is the residual between that and the density where the three layers
correspond to equal exposure — the layers' toes start at different exposures.
The datasheet fits carry exactly that term (blue −0.002…−0.101 by stock). With
the base left free the two are degenerate; with it measured, the offset is
identifiable in principle, which is why it was worth a render rather than an
argument.

**Verdict (user, all six frames):** `sigmoid-knees` is white on every white
surface; the datasheet pair is worst, strongly blue; the shipped gain and the
re-fitted pair sit between, and the re-fitted pair is not an improvement —
pinker on the July Ektar and Portra frames, slightly bluer on `ektar0909-1632`.
On asian skin the ranking reverses: the re-fitted pair is reddest, then the
shipped gain, then `sigmoid-knees`, which reads slightly green and worst.

**Measured on the same renders:**

| | white flag, G/R · B/R | deciles L1 → L9, B/R |
|---|---|---|
| `flat`, shipped gain | 1.084 · 1.089 | 1.76 → 1.48 |
| `flat`, re-fitted pair | 1.069 · 1.086 | 1.70 → 1.47 |
| `flat`, datasheet pair | 1.273 · 1.263 | — |
| `sigmoid-knees` | 1.024 · 1.027 | **1.65 → 1.27** |

(Encoded 8-bit, 18 % inset; the decile columns are one frame's own scene colour,
so only the *trend* across them is comparable.) The knee'd render's much steeper
fall was read at the time as its per-channel shoulder pulling the channels
together; that attribution did not survive (Appendix F). The cast's direction is per roll: blue on 2026-09-09-Ektar100, pink on
2026-07-15-Ektar100 and 2026-09-11-Portra400.

Frame-by-frame notes are in `../temp/notes/observations.md`.

## Appendix F — Superseded claims

Kept so a reader who meets the old number elsewhere knows it was retired, and
why.

- **"`sigmoid-knees` reads clean on every white surface *because* its per-channel
  shoulder pulls the channels together."** The convergence is measured; the cause
  was not. `sigmoid-flat` and `sigmoid-knees` differ in **four** ways at once —
  the shoulder, the anchor (`mid-at-dmax-fraction` 0.28 against 0.5),
  `print_exposure` (0 against 2.17) and `display_tone` (`none` against reinhard) —
  so nothing in that round isolates the shoulder, and the ranking that anointed
  the knee'd render was made by eye on an axis the reviewer reports being
  insensitive to.

  **Narrowed to two candidates on 2026-09-20, by structure rather than by a
  render.** Of the four differences, three cannot move a channel ratio at all:
  `print_exposure` is a scalar gain after the curve; the anchor factors out of
  `out_c = 10^(gamma·(scale_c·D_c − A))` as `10^(−gamma·A)`, identical on every
  channel; and **every nc display tone is luminance-preserving** — `sdr.rs:244-247`
  and `hdr.rs:550` curve one luminance and multiply all three channels by the
  resulting ratio, so `shoulder`, `reinhard` and `none` alike leave every ratio
  invariant. What remains is the **per-channel shoulder** and the **gamut map**,
  which converges radially near luminance 1.0 (`sdr.rs:249-266`) with a ceiling
  that follows the rendered luminance — so the tone choice reaches it indirectly
  even though the tone itself cannot. Separating those two is
  `nf-display-stages/gamut-map-share`'s (filed 2026-09-22). It was pointed at
  `nf-reconstruction/anchor-spike`, which is done and did not separate them — nothing
  turns the gamut map off by flag, so every desaturation measurement so far reads
  shoulder-plus-gamut-map jointly.

  **Separated on 2026-09-23: it is the per-channel shoulder.** Read on both sides of the
  map in float, `sigmoid-knees` has the map touch 0.00% of top-end pixels on all four
  measured rolls, and no marked white (`docs/reports/gamut-map-share.md`). The shoulder
  is the cause by elimination rather than by a matched render: removing it also lifts
  the whites by up to 60 L\*, so its convergence is not measured apart from its
  luminance compression.
- **"The Ektar green cast is a drift, not a hue."** The argument used
  `curve_probe::channel_drift`, which groups **ordinary picture pixels** by red
  density and reads the green/red ratio across the groups, with no grey patch. A
  constant scene colour cancels, but scene colour that correlates with
  brightness does not — and Hawaii frames (bright blue sky and sea over mid-tone
  foliage) are exactly that case, so the result is suggestive, not shown. Two
  points still stand: the Ektar sheet predicts +0.22 green drift where scans
  show +1.26, and `generic-c41` renders Ektar better than Ektar's own sheet.

- **"The sheet and the scanner disagree by a factor of two."** Written in the
  first draft of Part 1 and wrong: it compared the datasheets' **(gain,
  offset)** fit against our **gain-only** fit. Collapsed onto a zero-offset gain
  at our own patch densities the sheets imply ≈0.94 green and ≈0.80 blue, so the
  disagreement is ≈10 %.
- **The mid-slope figures "Ektar 0.64/0.60/0.76, Portra 400 0.52/0.57/0.63."**
  Computed by sampling each channel's table at its own index midpoint — but the
  channels have different point counts, so those are three different densities.
  The registry's own γ column is the number to use, and it puts every stock's
  green *above* red, not below.
- **Reconstruction as "the best estimate of relative per-channel exposure, per
  stock."** Part 1's first goal, replaced 2026-09-17 by the fixed stock-agnostic
  decode, because per-stock inversion normalizes tone character, inverts a toe
  that carries no information, and needs data that cannot exist for every stock,
  developer and scanner.
