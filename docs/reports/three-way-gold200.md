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
