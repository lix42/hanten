# What NLP and SilverFast actually do

Three outside conversions of one roll — **NLP**, **SilverFast** (SF) and **SilverFast
with CCR** (SFC) — measured against the source negatives. Roll `2026-09-18-Gold200`,
35 frames, 2026-09-20.

This is the first time nc's pipeline has been compared against commercial converters at
the level of *mechanism* rather than appearance. It exists to answer what they do, where
they agree, and which of their choices nc should or should not copy.

## Method

Everything is measured on the producers' own **16-bit Adobe RGB TIFFs**, never on the
sRGB JPEGs the review app displays, so no finding can be an artefact of that conversion.

The source negatives decode to density as `D = −log10(transmission / film_base)` with
this roll's measured base `[0.47095445, 0.23244068, 0.10803387]`. Where a frame is
pixel-aligned with its conversion (most are), the transfer curve is read directly as a
binned median of output against density. **A pure exponential decode is a straight line
in `log10(output)` against density**; curvature at either end is a toe or a shoulder.

Scripts and raw measurements: `../temp/gold200-3way/` (`curve.py`, `survey.py`,
`slopes.py`, `anchor.py`, `whitepin.py`, `noise.py`, `dstats.py`, `headroom.py`).

### Two limits on what can be concluded

**The per-channel slope comparison carries a systematic bias of about 0.05.** Measuring
nc's *own* `film-master` by this method recovers `G/R 0.784, B/R 0.696` where nc is
configured at `0.840 / 0.730` — the gap is the NC film RGB v1 → ACEScg matrix mixing
channels after the per-channel gain is applied. Every producer's output sits behind its
own matrix, so each carries its own version of this error and it cannot be removed
without knowing them. **Differences under ~0.05 in any ratio table below are inside the
method's error.**

**Slope is base-immune; level is not.** A film base is a per-channel *constant in
density*, so it moves a curve's intercept and not its gradient. This matters because
NLP gets its base indirectly through a Lightroom white balance and SilverFast never sees
one at all — every slope-based finding here survives that, every level-based one is
stated relative to our own measured base.

## Summary

- **They agree at white and diverge in the shadows.** The three transfer curves land
  within 0.02 log units of each other at the top of the density range and up to **1.4
  log units apart at the bottom**.
- **The neutral white is theirs, not our conversion's**, and it is not clipping.
- **None of them is an exponential.** All three are S-curves; NLP's toe is by far the
  strongest.
- **All three anchor the bright end on content**, at the 96–97th percentile of the
  frame's own density. None anchors at the frame's maximum, and none has leader data.
- **NLP forces white neutral per channel; SilverFast does not.** That single difference
  explains both NLP's reliability at the top and its worst failure elsewhere.
- **CCR is a film-base subtraction by lookup** — level only, no slope change.
- **The cost of content-adaptive contrast is noise**, and it tracks slope at r = 0.89.
- **nc today is not an S-curve at all** and runs at roughly half their contrast.

---

## What they share

### The white point is theirs, not ours

The top 0.1% of pixels reads neutral in the TIFF: chroma `C*` median **0.76** (NLP),
**2.27** (SF), **1.59** (SFC), with 33 of 35 frames under `C* = 6`. 16-bit clipping is
0.002% (NLP) to 0.043% (SF/SFC) of pixels, so this is not clipping into white — NLP in
particular reaches the most neutral white with the *least* clipping, which means it
computes a white point rather than running into one.

### None of them is an exponential

Residuals from a mid-range straight line, frame 1774:

| | mid-slope (R) | toe residual | shoulder residual | median R² over the roll |
|---|---|---|---|---|
| NLP | 4.44 | **−1.75** | −0.95 | 0.955 |
| SF | 4.19 | −0.34 | −0.72 | 0.989 |
| SFC | 4.21 | −0.17 | −0.78 | 0.988 |

NLP carries by far the strongest toe and is the least well described by a straight line
anywhere.

### Contrast is content-adaptive

Red slope correlates **−0.58 (NLP)**, −0.36 (SF), −0.22 (SFC) with the source's own
density span: a flatter negative gets more contrast. Roll-median red slope is 3.08 /
3.20 / 3.07.

None of them preserves the scene's own exposure — correlation between source median
density and delivered median L\* is **+0.08 to +0.13**, i.e. none. Every frame is
normalised. Delivered L\* spans 31–91 across the roll for a source density range of only
0.40–0.58.

### Where the anchor is

Expressing the density at which the output reaches a reference level as a **percentile
of that frame's own density distribution**, then asking how stable that percentile is
across the roll (green channel, sd over frames):

| | near-white (0.90) | mid (0.18) | dark (0.02) |
|---|---|---|---|
| NLP | 95.6% ± **6.1** | 30.4% ± 16.4 | 11.6% ± 8.0 |
| SF | 96.8% ± **2.5** | 35.6% ± 19.3 | 18.6% ± 9.1 |
| SFC | 96.8% ± **2.6** | 35.7% ± 19.1 | 15.4% ± 9.9 |

**All three anchor the bright end, on content, at about the 96–97th percentile.** The mid
floats (sd 16–19). SilverFast is the most consistent about it. Nobody anchors at the
frame's maximum — the gap above the anchor is deliberate headroom for speculars to fold
into.

### Per-channel slopes

On frame 1774, with the ±0.05 method bias above in mind:

| | G/R | B/R |
|---|---|---|
| nc `film-master`, measured | 0.784 | 0.696 |
| nc, configured | 0.840 | 0.730 |
| NLP | 0.826 | 0.757 |
| SF | 0.835 | 0.643 |
| SFC | 0.848 | 0.653 |

What survives the error bar: **everyone's green sits in 0.78–0.85**, and **blue
genuinely splits** — NLP high (0.757), SilverFast low (0.643–0.653), a 0.11 gap larger
than the bias. nc's blue sits between them. Over the whole roll the ratios scatter
widely, so this is agreement on typical frames, not a constant they share.

---

## Where they differ

### NLP forces white; SilverFast does not

The difference shows in the *distribution* rather than the median. NLP's top-end chroma
is **bounded at 3.73** across all 35 frames (p25 0.09, p90 2.95); SF's tails to **9.04**
and SFC's to 6.45. A hard ceiling with no tail is what forcing looks like.

Per-frame variability says the same. Coefficient of variation across the roll:

| | red slope | G/R ratio | B/R ratio |
|---|---|---|---|
| NLP | 52.6% | 31.6% | **65.5%** |
| SF | 24.6% | 25.2% | 35.7% |
| SFC | 24.5% | **18.8%** | **28.4%** |

NLP re-fits colour as hard as it re-fits contrast. SFC barely moves colour at all — what
a fixed per-stock calibration looks like.

The decisive measurement is the output at **each channel's own 97th density percentile**.
Pinning every channel to a common white makes these equal:

| | R | G | B | spread |
|---|---|---|---|---|
| NLP | 0.863 | 0.884 | 0.866 | **1.08×** |
| SF | 0.828 | 0.763 | 0.726 | 1.13× |
| SFC | 0.836 | 0.755 | 0.751 | 1.12× |

So **SF anchors the bright end in *tone*, not in *colour*.** Its channels land unequal
and consistently ordered R > G > B; they only converge at the very top (1.05× at the
99.5th percentile) because the shoulder and the ceiling bring them together. NLP makes
white neutral by construction.

**The trade: NLP never fails at white and can fail catastrophically away from it;
SilverFast fails mildly at white and degrades gracefully.**

### How NLP picks its per-channel slopes

Two hypotheses are ruled out by measurement:

- **Not per-channel range normalisation.** If each channel's own range were mapped to
  the output range, `slope_c × span_c` would be equal across channels. It differs by a
  median of **48%** (SF 35%, SFC 28%).
- **Not grey-world.** Frame-mean `a*` has sd 8.25 (NLP), 5.13 (SF), 2.36 (SFC) across
  the roll — nothing is driven to an average neutral. A single-colour scene stays
  coloured under all three.

What *is* pinned is the highlight, **per channel**: the spread of the near-white anchor
percentile across R/G/B within a frame is **2.3 percentile points** for NLP — tighter
than any other point, for any tool. NLP sets each channel's slope so that channel's own
highlight lands on white, and the per-channel differences fall out of where those
highlights sit.

### CCR is SilverFast's substitute for measuring the film base

Isolating SFC against SF over 16 aligned frames, CCR changes **slope not at all**
(R 1.007×, G 1.007×, B 1.001×; G/R ratio +0.019) and **level substantially** (median
Δa\* +2.55, Δb\* −4.13, ΔL\* −0.10). It cuts SF's mean cast `|a*| + |b*|` from **12.8 to
4.9**.

A pure per-channel level correction keyed to the stock *is* a film-base subtraction by
lookup. SilverFast never reads the rebate, so CCR supplies from the datasheet what nc
measures directly. What that costs is measurable:

| Gold 200 mask, relative to red | G−R | B−R |
|---|---|---|
| datasheet E-7022 Status M `d_min` | +0.4054 | +0.7396 |
| our measured base, this roll | +0.3067 | +0.6394 |
| **gap** | **−0.0987** | **−0.1002** |

The published mask has the right *shape* and the wrong *level*, by a constant **−0.10
density on both G and B** — this scanner reads ~0.10 more density in red on the base
than Status M does. That the two gaps agree to 0.0015 makes it a single systematic term,
not stock variation, and a measured starting point for `io/scanner-density-calibration`.

---

## Where it goes wrong

### The two frames that lose the neutral white

**1799** and **1817**, both SilverFast's failures — NLP holds white on 34 of 35. The
predictor is a property of the *negative*, computable before any render:

> dark-end channel spread ÷ usable density span

1799 ranks 1st (0.269) and 1817 4th (0.134); correlation with the worst top-end chroma is
+0.39, and with NLP's whole-frame cast +0.41. A frame scoring high has no place where
all three channels sit near the base, so a content-fitted per-channel correction has no
dark anchor and is extrapolating. 1817 additionally has no bright content at all
(p99 L\* ≈ 89 against the roll's 96–97).

### Frame 1799 exposes NLP's method

NLP's **top is neutral** (a\* −0.44) while its **whole frame is a\* −21.3** — white right,
everything else strongly green. Its G/R slope ratio collapses from 0.826 (frame 1774) to
**0.624**, with green's intercept **+1.55 log units above red**: green starts high and
rises slowly, so shadows go green while highlights converge to neutral.

1799's channel dark-ends are spread by 0.123 density, the widest on the roll. Pin three
highlights to a common white while the dark ends disagree and the three slopes must
diverge to absorb it — and the error lands where the anchor is not, which is the shadows.
It is the same structure behind the long-standing observation that NLP renders highlights
cleanly and casts in the shadows.

Measured whole-frame cast reproduces the visual ranking exactly (SFC best, SF blue, NLP
green): C\* **7.8 / 14.7 / 21.4**.

### The cost of content-adaptive contrast is noise

Measured at full resolution on a central crop, as a low percentile of `|I(x+1) − I(x)|`
(flat areas dominate the low percentiles, so this reads the grain/scanner floor rather
than scene detail):

| | corr(slope, noise) | corr(source density span, noise) |
|---|---|---|
| NLP | **+0.89** | −0.46 |
| SF | +0.71 | −0.59 |
| SFC | +0.66 | −0.51 |

**Noise gain follows the slope, essentially one-for-one.** A narrow negative gets more
contrast, and the contrast multiplies the scanner's own noise with the signal. The
negatives themselves are uniform — the scan's noise floor differs by only 1.01× between
the roll's thinnest and thickest frames — so everything above that is amplification the
tool added.

**Brightening is not what costs.** Correlation between how far a frame was *lifted*
(delivered L\* above what its own source density predicts) and its delivered noise is
**−0.49** for all three: the most-lifted frames are the *least* noisy. Level is free;
slope is not.

Worst cases, as a multiple of that scan's own noise floor:

| frame | source span | NLP | SF | SFC | NLP slope |
|---|---|---|---|---|---|
| 1799 | 0.442 | **4.1×** | 2.4× | 2.3× | 8.14 |
| 1813 | 0.408 | 3.5× | 2.6× | — | 7.10 |
| 1800 | 0.445 | 2.6× | 2.9× | 2.9× | 3.87 |
| 1817 | 0.456 | 2.5× | 2.0× | 2.0× | 3.84 |

Five of the six noisiest frames are the five narrowest negatives.

---

## nc compared

### nc today is not an S-curve, and is far flatter

Same frame, same method, nc's own renders:

| | mid-slope (R) | R² | toe residual | shoulder residual |
|---|---|---|---|---|
| nc `film-master` | 2.19 | 0.9908 | +0.05 | −0.08 |
| nc `sigmoid-flat` | 1.72 | 0.9986 | −0.02 | −0.13 |
| nc `sigmoid-knees` | 2.02 | 0.9991 | +0.03 | −0.13 |
| NLP / SF / SFC | 4.44 / 4.19 / 4.21 | 0.945–0.990 | −1.75 / −0.34 / −0.17 | −0.95 / −0.72 / −0.78 |

Every nc configuration measures as a **straight line** — even `sigmoid-knees`, whose
knees this frame's density range never reaches. Normalised to a common mid-point, nc puts
the darkest content 0.58 log units below mid where the tools put it **1.5–2.7** below.
nc and the tools broadly agree in the highlights and differ enormously in the shadows —
the same structure as tool-against-tool, with nc at the extreme.

The S-shape has to come from somewhere in the new chain, and the design already says
where: a straight decode plus a fit-range operator with a toe and a shoulder composes to
exactly this shape.

### How much headroom a white anchor would leave

If nc anchors diffuse white, everything above it is what an HDR rendition carries and an
SDR shoulder folds. Stops above a candidate anchor, to each frame's p99.99, at gamma 2.0:

| anchor | median | p90 | max | frames under 0.5 stop |
|---|---|---|---|---|
| p95 | 1.23 | 2.01 | 2.51 | 3 of 35 |
| p97 | 1.03 | 1.78 | 2.22 | 5 of 35 |
| p99 | 0.67 | 1.33 | 1.77 | 8 of 35 |

Against the 2.30 stops the containers carry (1000/203), a p97 anchor leaves about a stop
on a typical frame. The shipped default leaves none — the reference-anchored sigmoid
rolls its shoulder so diffuse white lands *at* reference white, which is why
`gain-map-hdr` writes a `GainMapMax` of 1.0×. Details and the trap in
`docs/tasks/nf-reconstruction/anchor-rule.md`.

---

## What nc can take from this

- **Their agreement is at white and their disagreement is in the shadows.** Judging a
  converter on white patches compares the part they all solve. Midtone and shadow
  neutrals are what separate them — the same conclusion the 2026-09-17 offset round
  reached from the other direction.
- **nc's structure is SilverFast's, with two advantages.** One common stretch plus a
  fixed per-channel calibration is what nc already does — and nc *measures* the film
  base where CCR looks it up, on a lookup that is 0.10 density off for this stock. nc
  also does not re-fit colour per frame, which is the mechanism behind NLP's worst
  failure.
- **Per-channel white pinning is the thing not to copy.** It is NLP's method, and on a
  frame with no common dark reference it produces a\* −21 while keeping the top neutral.
  Evidence for nc's `Dmax` staying a **scalar** anchor.
- **Two source-side warnings are cheap and nc has neither.** Dark-end channel spread ÷
  density span says when a fitted per-channel correction is extrapolating; density span
  says how far contrast can stretch before noise shows. Both are `nc inspect` facts
  computable before a pixel is rendered, and "fail loudly" argues for reporting them —
  none of these tools tells you when it is guessing.
- **The "bounded" requirement now has a number.** An opt-in scene-range mapping's
  ceiling is a **noise budget** derived from the negative's own density span, not a
  constant picked by taste. Input to `nf-look/scene-range-mapping`.
- **nc is much flatter than all three** (2.0 against ~3.1). Worth stating whenever an nc
  render is compared against these: it is a look difference before it is a colour
  difference.
- **CCR measurably works** — roll-median whole-frame cast SFC **3.8**, NLP 7.4, SF 10.9 —
  and it works by doing what nc already does better.
- NLP is the most saturated: 1.33% of its pixels fall outside sRGB against 0.49%
  (SF/SFC), so a review set's sRGB JPEGs clip NLP hardest. Plausibly a deliberate look,
  and a candidate for an opt-in look profile rather than a default.

## What this cannot say

- **Whose per-channel numbers are right.** The ±0.05 matrix bias makes green
  indistinguishable across all four. nc's own 31 marked patches remain the better
  evidence, and they call blue solid at 0.68–0.78 on every roll measured.
- **Whether any of it generalises past one stock and one scanner.** Every number here is
  Gold 200 on one Epson. Collecting SilverFast conversions across stocks would test the
  central claim directly: if SF's per-channel slopes hold while CCR's offset moves with
  the stock, the split between fixed calibration and stock lookup is confirmed.
- **What the tools do internally.** Everything is inferred from input and output. A
  per-channel gain followed by a matrix and a different gain followed by a different
  matrix can produce the same pixels.
