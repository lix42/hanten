# Where to put the decode's white

Four candidate rules for placing white in the decode, costed from the scans alone.
Produced by `nf-reconstruction/anchor-spike`, 2026-09-21; the rendered ranking is
`nf-calibration/anchor-comparison` and is deliberately not here.

Measured on three rolls — `2026-09-18-Gold200` (35 frames), `2026-09-14-Ektar100` (32)
and `2026-09-11-Portra400` (12). Scripts and raw data in `../temp/anchor-spike/`.

## Method

Densities are `D′ = scale · D` with the shipped `[1, 0.84, 0.73]`, read on **red**,
where `scale = 1` and `D′ = D` — the units the anchor is stated in. Bases measured per
roll with `estimate --grid` (cell spreads 0.007–0.021, benign gradients). A 12% inset
keeps the film holder out of the high percentiles; it renders *bright*, not dark, and
will otherwise dominate them.

`W` denotes the roll's white candidate: the red p97 taken toward the roll's upper end.

## The framing correction

`AnchorPlacement::anchor` resolves `mid-at-base-offset(d)` to
`d + MID_GREY_OUTPUT_DECADES / contrast`, and that value **is** the density where output
reaches 1.0. At `d = 0.62`, `contrast = 2.0` it is **0.9924** — confirmed against the
binary, which reports `anchor_value: 0.99236375` — against the datasheets' diffuse white
at `d + REFERENCE_MID_TO_WHITE_DELTA` = **0.98**. A gap of 0.012 density, ~0.08 stops.

**nc already anchors at diffuse white**, spelled as a mid anchor. On a straight line
"mid anchor" and "white anchor" name one rule, so the question is not where on the scale
to pin but **what the pin is referenced to**.

## The base-referenced rule is open-loop

It says where white *would* be if the roll met the aim, and never checks that it did:

| roll | n | red p97 | vs anchor | renders at | stops short |
|---|---|---|---|---|---|
| 2026-09-18-Gold200 | 35 | 0.800 | −0.193 | 0.412 | **−1.28** |
| 2026-09-14-Ektar100 | 32 | 0.910 | −0.083 | 0.683 | −0.55 |
| 2026-09-11-Portra400 | 12 | 0.860 | −0.133 | 0.543 | −0.88 |

**Nothing reaches the region a per-channel highlight operator acts in**, so such an
operator would be inert under this decode whatever form it takes. That is what couples
the anchor question to `nf-look/path-to-white` by a number rather than an argument.

**And one fixed `d` cannot serve the three.** Reaching white would need `d` = 0.428 /
0.538 / 0.488 — a spread of 0.110 density, **0.73 stops**. So "0.62 is too high" is not
the reading: any single value leaves the rolls that far apart at the top.

Per-frame would be far worse — within-roll spread of per-frame red p97 is 0.40 / 0.40 /
**0.96**. Roll is the granularity; per frame is NLP's method and its failure mode.

## The shortlist

On Gold200, where `W` = 0.800 and `d` = 0.62:

| | gamma | anchor `A` | a datasheet mid renders at | white reached |
|---|---|---|---|---|
| **A** fixed anchor — `mid-at-base-offset(d)` | 2.00 | 0.992 | **0.180** ✓ | 0.412 ✗ |
| **B** content white — `A = W`, a level move | 2.00 | 0.800 | 0.437 ✗ | **1.000** ✓ |
| **C** hybrid — pin mid *and* solve contrast | 4.15 | 0.800 | **0.180** ✓ | **1.000** ✓ |
| **D** C with a gamma ceiling, sliding toward B when it binds | capped | — | ✓ | mostly |

**C is the only one that pins both ends**, and structurally: two free parameters
(anchor and contrast) against two constraints, where A and B each have one. Its contrast
follows from `gamma = MID_GREY_OUTPUT_DECADES / (W − d)` — **4.15 / 2.57 / 3.11** on the
three rolls.

**B is weaker than "level is free" suggests.** A level move puts a true datasheet
mid-grey at 0.437 instead of 0.18, because it assumes the roll's brightest *is* white
when the roll may simply be flat.

**C stays inside the design.** `gamma` already splits — linearization (~1.8) in the
decode, print contrast in rendering — so C's per-roll contrast is the **look stage's
contrast knob** (2.31× / 1.43× / 1.73× over 1.8) and the decode stays fixed and
stock-agnostic. The roll-level remainder landing in rendering is what design-update
already says should happen.

**C's cost is noise, and it is not small.** `corr(slope, noise) = 0.89`
(`three-way-gold200.md`), and C asks Gold200 for gamma 4.15 where the converters
measured 3.07–3.18 on that same roll — because its content spans only 0.18 density from
`d` to its bright end where the datasheets expect 0.36. **C cannot tell a genuinely flat
roll from a wrong `d`, and pays for either in contrast.** Hence D, which makes the noise
budget an explicit parameter instead of an accident.

## Two guards on the percentile, pulling opposite ways

A **high percentile rather than the maximum**, or one blown frame drags the anchor up
and darkens the whole roll. And a margin leaving the measured **~1 stop of specular
headroom** above white, as well as distance below the leader's `Dmax`.

## What none of this decides

Whether an underexposed roll *should* be lifted. A content-referenced anchor normalises
roll-level exposure, so two rolls of one scene at different exposures render alike —
"show what the negative really holds" giving ground. That is a product decision, and no
measurement here reaches it.

## Aside: the holder renders above white

`--display-tone none` refused a Gold200 frame outright because the **film holder**
renders above reference white — opaque, so high density, so bright in the positive.
`effective_area` reported `holder: 0` on all four edges with `holder_applied: false`,
applying only its 5% static inset (167 px of 3343), which does not reach the holder on
this scan. Relevant to `film-base/holder-cap-contamination`, and a trap for any
measurement taken on a full frame rather than an inset one.
