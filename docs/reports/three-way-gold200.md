# What NLP and SilverFast actually do — 2026-09-18-Gold200

Three outside conversions of one roll (35 frames), measured against the source
negatives: **NLP**, **SilverFast** (SF) and **SilverFast with CCR** (SFC). Scripts and
raw measurements in `../temp/gold200-3way/`.

Everything below is measured on the producers' **16-bit Adobe RGB TIFFs**, not on the
sRGB JPEGs the review app shows, so nothing here can be an artefact of that step.

## The headline

**They agree at white and diverge in the shadows.** On a pixel-aligned frame the three
transfer curves land within 0.02 log units of each other at the top of the density
range and up to **1.4 log units apart at the bottom** — NLP crushing hardest. Every
tool anchors the bright end; none of them agrees about black.

## The white point is theirs, not ours

The top 0.1% of pixels reads neutral in the TIFF: chroma `C*` median **0.76** (NLP),
**2.27** (SF), **1.59** (SFC), on 33 of 35 frames under `C* = 6`. 16-bit clipping is
0.002% (NLP) to 0.043% (SF/SFC) of pixels, so this is not clipping into white — NLP in
particular reaches the most neutral white with the *least* clipping, which means it is
computing a white point rather than running into one.

## None of them is an exponential

A pure exponential decode is a straight line in `log10(output)` against density. None
of these is. On frame 1774, residuals from a mid-range straight line:

| | mid-slope (R) | toe residual | shoulder residual | median R² over the roll |
|---|---|---|---|---|
| NLP | 4.44 | **−1.75** | −0.95 | 0.955 |
| SF | 4.19 | −0.34 | −0.72 | 0.989 |
| SFC | 4.21 | −0.17 | −0.78 | 0.988 |

NLP carries by far the strongest toe and is the least well described by a straight line
anywhere. All three run **more contrast than nc**: roll-median red slope 3.08 / 3.20 /
3.07 against nc's fixed `gamma = 2.0`.

## Contrast is content-adaptive, per channel

Red slope correlates **−0.58 (NLP)**, −0.36 (SF), −0.22 (SFC) with the source's own
density span: a flatter negative gets more contrast. The per-channel ratios move with
it, so these are fitted per frame, not a fixed calibration.

On a *typical* frame they nevertheless bracket nc's calibrated `density.scale`:

| frame 1774 | G/R | B/R |
|---|---|---|
| NLP | 0.826 | 0.757 |
| SF | 0.835 | 0.643 |
| SFC | 0.848 | 0.653 |
| **nc `density.scale`** | **0.840** | **0.730** |

**This comparison is base-immune.** A film base is a per-channel *constant in density*,
so it moves the intercept and not the gradient — which matters because NLP gets its
base indirectly through a Lightroom white balance and SF never sees one at all. Over
the whole roll the ratios scatter widely (NLP most), so read this as agreement on
typical frames, not a shared constant.

**But the method carries a systematic bias of about 0.05, so read the table loosely.**
Measuring nc's *own* `film-master` the same way recovers **0.784 / 0.696** where nc is
configured at 0.840 / 0.730 — the gap is the NC film RGB v1 → ACEScg matrix mixing
channels after the per-channel gain is applied. Every producer's output sits behind its
own matrix, so each carries its own version of this error and it cannot be removed
without knowing them. On a like-for-like measured basis:

| measured the same way | G/R | B/R |
|---|---|---|
| nc `film-master` | 0.784 | 0.696 |
| NLP | 0.826 | 0.757 |
| SF | 0.835 | 0.643 |
| SFC | 0.848 | 0.653 |

Differences under ~0.05 are inside the method's error. What survives it: **everyone's
green sits in 0.78–0.85**, and **blue genuinely splits** — NLP high (0.757), SilverFast
low (0.643–0.653), a 0.11 gap that is larger than the bias. nc's blue sits between them.

## Which frames lose the neutral white, and why

Two of 35: **1799** and **1817**, both SilverFast's failures (NLP holds white on 34/35).
The predictor is a property of the *negative*, computable before any render:

> dark-end channel spread ÷ usable density span

1799 ranks 1st (0.269) and 1817 4th (0.134); correlation with the worst top-end chroma
is +0.39, and with NLP's whole-frame cast +0.41. A frame scoring high has no place where
the three channels are simultaneously near the base — so a content-fitted per-channel
correction has no dark anchor and is extrapolating. 1817 additionally has no bright
content at all (p99 L\* ≈ 89 against the roll's 96–97).

## Frame 1799 exposes NLP's method

NLP's **top is neutral** (a\* −0.44) while its **whole frame is a\* −21.3** — white right,
everything else strongly green. Its G/R slope ratio collapses from 0.826 (frame 1774) to
**0.624**, with green's intercept sitting **+1.55 log units above red**: green starts high
and rises slowly, so shadows go green while highlights converge to neutral.

That is the signature of **per-channel curves fitted from content and anchored at the
white end**. 1799 is the frame where that fit has least to work with, and it fails in the
half of the tone scale furthest from its anchor. It is the same structure behind the
long-standing observation that NLP renders highlights cleanly and casts in the shadows.

Measured whole-frame cast reproduces the visual ranking exactly (SFC best, SF blue, NLP
green): C\* **7.8 / 14.7 / 21.4** for SFC / SF / NLP.

## NLP forces white; SilverFast does not

This is the difference behind 1799, and it shows in the *distribution* rather than the
median. NLP's top-end chroma is **bounded at 3.73** across all 35 frames (p25 0.09,
p90 2.95); SF's has a tail to **9.04** and SFC's to 6.45. A hard ceiling with no tail is
what forcing looks like; a tail is what leaving it where it lands looks like.

The per-frame variability says the same thing. Coefficient of variation across the roll:

| | red slope | G/R ratio | B/R ratio |
|---|---|---|---|
| NLP | 52.6% | 31.6% | **65.5%** |
| SF | 24.6% | 25.2% | 35.7% |
| SFC | 24.5% | **18.8%** | **28.4%** |

NLP re-fits colour as hard as it re-fits contrast. SFC barely moves colour at all — which
is what a fixed per-stock calibration looks like. So the trade is: **NLP never fails at
white and can fail catastrophically away from it; SilverFast fails mildly at white and
degrades gracefully.**

## CCR is SilverFast's substitute for measuring the film base

Isolating SFC against SF over 16 aligned frames, CCR changes **slope not at all**
(R 1.007x, G 1.007x, B 1.001x; G/R ratio +0.019) and **level substantially**
(median delta a\* +2.55, delta b\* -4.13, delta L\* -0.10). It cuts SF's mean cast
`|a*| + |b*|` from **12.8 to 4.9**.

A pure per-channel level correction, keyed to the stock, is a *film-base subtraction by
lookup*. SilverFast never reads the rebate, so CCR supplies from the datasheet what nc
measures directly. What that costs is now measurable:

| Gold 200 mask, relative to red | G-R | B-R |
|---|---|---|
| datasheet E-7022 Status M `d_min` | +0.4054 | +0.7396 |
| our measured base, this roll | +0.3067 | +0.6394 |
| **gap** | **-0.0987** | **-0.1002** |

The published mask has the right *shape* and the wrong *level*, by a constant **-0.10
density on both G and B** — i.e. this scanner reads ~0.10 more density in red on the
base than Status M does. That the two gaps agree to 0.0015 makes it a single systematic
term, not stock variation, and it is a measured starting point for
`io/scanner-density-calibration`.

## nc today is not an S-curve, and is far flatter than any of them

Same frame, same method, nc's own renders:

| | mid-slope (R) | R² | toe residual | shoulder residual |
|---|---|---|---|---|
| nc `film-master` | 2.19 | 0.9908 | +0.05 | -0.08 |
| nc `sigmoid-flat` | 1.72 | 0.9986 | -0.02 | -0.13 |
| nc `sigmoid-knees` | 2.02 | 0.9991 | +0.03 | -0.13 |
| NLP / SF / SFC | 4.44 / 4.19 / 4.21 | 0.945-0.990 | -1.75 / -0.34 / -0.17 | -0.95 / -0.72 / -0.78 |

Every nc configuration measures as a **straight line** — even `sigmoid-knees`, whose
knees the frame's density range never reaches. Normalised to a common mid-point, nc puts
the darkest content 0.58 log units below mid where the tools put it **1.5-2.7** below.
nc and the tools broadly agree in the highlights (+0.44 to +0.69 at the top) and differ
enormously in the shadows — the same structure as tool-against-tool, with nc at the
extreme.

So the S-shape those tools carry has to come from somewhere in the new chain. The design
already says where: a straight decode plus a fit-range operator with a toe and a
shoulder composes to exactly this shape. This measurement is the argument for that
operator existing, and a target for what it has to do.

## Where each tool anchors, and how NLP picks its per-channel slopes

Expressing the density at which the output reaches a reference level as a **percentile
of that frame's own density distribution**, then looking at how stable that percentile
is across the roll (green channel, sd over frames):

| | near-white (0.90) | mid (0.18) | dark (0.02) |
|---|---|---|---|
| NLP | 95.6% ± **6.1** | 30.4% ± 16.4 | 11.6% ± 8.0 |
| SF | 96.8% ± **2.5** | 35.6% ± 19.3 | 18.6% ± 9.1 |
| SFC | 96.8% ± **2.6** | 35.7% ± 19.1 | 15.4% ± 9.9 |

**All three anchor the bright end, on content, at about the 96–97th percentile.** The mid
floats (sd 16–19). SilverFast is the most consistent about it (sd 2.5), so yes — SF maps
content to its tone placement, and the anchor is the highlight, not the mid.

Two hypotheses for NLP's per-channel slopes are ruled out by measurement:

- **Not per-channel range normalisation.** If each channel's own range were mapped to the
  output range, `slope_c x span_c` would be equal across channels. It differs by a median
  of **48%** (SF 35%, SFC 28%).
- **Not grey-world.** Frame-mean `a*` has sd 8.25 (NLP), 5.13 (SF), 2.36 (SFC) across the
  roll — nothing is being driven to an average neutral.

What *is* pinned is the highlight, **per channel**: the spread of the near-white anchor
percentile across R/G/B within a frame is **2.3 percentile points** for NLP — tighter than
any other point, for any tool. So NLP sets each channel's slope so that channel's own
highlight lands on white, and the per-channel differences fall out of wherever those
highlights sit.

That closes the loop on 1799. Its channels' **dark** ends are spread by 0.123 density, the
widest on the roll. Pin three highlights to a common white while the dark ends disagree
and the three slopes must diverge to absorb it — G/R falls to 0.624, and the error lands
where the anchor is not, which is the shadows.

**The clean distinction between the two products:**

- **SilverFast** — content sets the *overall* tone placement (one anchor, bright end);
  per-channel balance comes from a fixed calibration plus CCR's stock-keyed offset.
- **NLP** — content sets tone placement *and* per-channel balance, each channel pinned
  to white independently.

nc's design is SilverFast's shape, with two differences in nc's favour: it **measures**
the base rather than looking it up, and it does not re-fit colour per frame. The one
place nc differs structurally is the anchor — nc's is a reference-free *mid* anchor,
where all three of these anchor the *highlight* on content.

## The cost of content-adaptive contrast is noise, and it is measurable

Per-frame fitting has a price. Measured at full resolution on a central crop, as a low
percentile of `|I(x+1) - I(x)|` (flat areas dominate the low percentiles, so this reads
the grain/scanner floor rather than scene detail):

| | corr(slope, noise) | corr(source density span, noise) |
|---|---|---|
| NLP | **+0.89** | -0.46 |
| SF | +0.71 | -0.59 |
| SFC | +0.66 | -0.51 |

**Noise gain follows the slope, essentially one-for-one.** The chain is: a narrow
negative gets more contrast, and the contrast multiplies the scanner's own noise with
the signal. The negatives themselves are uniform — the scan's noise floor differs by
only 1.01x between the roll's thinnest and thickest frames — so everything above that is
amplification the tool added.

**Brightening is not what costs.** Correlation between how far a frame was *lifted*
(delivered L\* above what its own source density predicts) and its delivered noise is
**-0.49** for all three: the most-lifted frames are the *least* noisy. Level is free;
slope is not.

Worst cases on this roll, as a multiple of that scan's own noise floor:

| frame | source span | NLP | SF | SFC | NLP slope |
|---|---|---|---|---|---|
| 1799 | 0.442 | **4.1x** | 2.4x | 2.3x | 8.14 |
| 1813 | 0.408 | 3.5x | 2.6x | — | 7.10 |
| 1800 | 0.445 | 2.6x | 2.9x | 2.9x | 3.87 |
| 1817 | 0.456 | 2.5x | 2.0x | 2.0x | 3.84 |

Five of the six noisiest frames are the five narrowest negatives. None of the three
preserves the scene's own exposure — correlation between source median density and
delivered median L\* is +0.08 to +0.13, i.e. none — so every frame is normalised and the
thin ones pay for it.

This gives the design's "bounded" requirement a number. An opt-in scene-range mapping
needs a ceiling on its gain, and that ceiling is a **noise budget** computable from the
negative's own density span before any pixel is rendered — not a constant picked by
taste.

## What nc can take from this

- **Their agreement is at white and their disagreement is in the shadows.** Judging a
  converter on white patches therefore compares the part they all solve. Midtone and
  shadow neutrals are what separate them — the same conclusion the 2026-09-17 offset
  round reached from the other direction.
- **CCR measurably works.** Roll-median whole-frame cast: SFC **3.8**, NLP 7.4, SF 10.9.
- **The warning condition above is cheap and nc has nothing like it.** A frame with no
  common dark anchor is one where any per-frame or per-roll fitted correction is
  extrapolating, and nc's "fail loudly" principle argues for reporting it rather than
  silently fitting. Candidate for `nf-look/scene-range-mapping`'s ceiling, and a
  prerequisite for any opt-in per-frame correction.
- **nc is much flatter than all three** (2.0 against ~3.1). Worth stating whenever an
  nc render is compared against these, because it is a look difference before it is a
  colour difference.
- NLP is the most saturated: 1.33% of its pixels fall outside sRGB against 0.49%
  (SF/SFC), so the review set's sRGB JPEGs clip NLP hardest.
