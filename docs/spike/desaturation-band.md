# Placing the desaturation band

Produced by `nf-look/desaturation-band-fit`, 2026-09-23. Set, scripts, raw measurements
and the throwaway operator (`operator.patch`) in `../temp/band-fit/` (uncommitted — the
frames are the user's photographs). Follows [`highlight-desaturation.md`](highlight-desaturation.md),
which settled the operator's form and fitted its band to one patch.

## The question

The chroma pull that `nf-look/path-to-white` ships is keyed on brightness *and* on a
saturation band: full pull below `s0`, none above `s1`. The spike placed it at
0.30 → 0.45 of linear-RGB `(max − min)/max` from one saturated patch on one roll. Where
should it sit, on what measure, and does one placement serve every roll?

## Method

- **Control:** candidate C from [`white-placement.md`](white-placement.md), by flags —
  `--density-curve exponential --anchor-mid-offset 0.62 --density-gamma g`, with `g`
  solved per roll so the roll's white `W` (p90 over frames of per-frame red p97) renders
  at 1.0: Gold200 09-18 **4.15**, Ektar100 09-14 **2.57**, Portra400 09-20 **2.32**.
- **Patches:** 52 — 40 marked by the user in the review app as `w` (should come out
  clean) or `c` (must keep its colour), plus the spike's 12. Only the **29 at or above
  the brightness start** (1 stop below diffuse white) count; below it the operator does
  nothing, so they cannot place the band. They are 12 W (Gold200, Ektar) and 17 C
  (12 Ektar, 3 Gold200, 2 Portra).
- **Measured at the operator's input:** `film-master` (linear ACEScg, before any print
  control), median per patch, then multiplied by each candidate white balance. Every
  saturation figure below is computed on **film RGB** — the white-balanced ACEScg value
  through the inverse of the v1 matrix — in the fit and in the operator alike.
- **Pair check:** a throwaway operator after the shared print controls, rendered on the
  22 frames holding in-range patches, judged by eye. Every control but the white balance
  sat at its identity (exposure 0, black point 0, `linear_range` `[0, 1]`), so the
  operator saw exactly the values the band was fitted on.

## The band measures distance from the roll's white

That is the main result, and it makes **white balance a precondition** of the operator
rather than a neighbour. Best single threshold, misclassified of 29, on the best measure:

| white balance ahead of the operator | misclassified | why |
|---|---|---|
| none (the new chain's default) | 6 | Ektar whites carry as much cast (C\* 16–38) as skin, sand and a leaf |
| per-frame auto (`--auto-wb`) | 9–10 | reads a sunset as the cast and removes it before the band sees it |
| per-roll, pooled top percentile | 2–6 | depends on the estimator; see below |
| per-roll, fitted to the W patches (a reference) | 3 | one band nearly fits both rolls |

Without a roll-level white balance there is no placement for Ektar that cleans its
whites without flattening skin.

**The white balance should keep the scene's light** (the user's framing): there is no
correct WB, only an intent, and one global gain cannot keep a sunset on the sky while
removing it from a cloth. So it removes only what is constant across a roll — film,
process and scanner — which a roll-level statistic estimates and one sunset frame barely
moves.

**The film cannot supply that white.** The base is the decode's per-channel divisor, so
it already neutralises black, where a gain does nothing. The leader renders 1.5–3.5 stops
above diffuse white and asks for gains the wrong way round (red 0.53–0.67, blue 1.8–2.6,
against the W patches' red 1.11–1.27, blue 1.18–1.65).

**Estimator:** the pooled per-channel p99 of the roll's pixels, excluding any pixel within
0.1 density of the leader (a guard against a fully exposed frame on the roll), is what the
pair check used. With the median of per-frame p95 gains (today's `--auto-wb percentile`,
per frame), Ektar's top 5% is sky and its blue gain comes out 0.79 against the W patches'
1.28; the median of per-frame p99 gains gives 1.16–1.21, and the pooled p99 used here
1.25. No variant clearly beats the others at 29 patches,
and the leader guard is untested — none of these rolls holds a blown frame.

## The measure: log ratio over gamma

`s = log10(max/min) / gamma` on film RGB is the negative's own density spread, and it
separates best under every white balance. **Linear RGB never separates across rolls**,
because the per-roll contrast rescales it: a fixed cast `Δd` renders at
`1 − 10^(−gamma·Δd)`, so Gold200's whites read 0.33–0.52 and Ektar's 0.20–0.34 with no
white balance. The spike's 0.30 → 0.45 does not carry — it was fitted under a per-frame
level anchor at a different contrast, as the task expected. A perceptual `C*/L*` falls
between the two.

## The distribution, and the placement

In-range patches, `s` under the per-roll white balance (`?` = the user was unsure; a
5–6 PM cloud, reported but not fitted). Twelve characters per 0.02; a digit is that many
patches in one column; values above 0.10 are listed at the end:

```text
s_log  0.00        0.02        0.04        0.06        0.08        0.10
       |           |           |           |           |           |
W        ● ●    ●●●● ●●  ●  ●   ●              ●
C                       ●   ● ● ●           ●●  ●●   ●2  ●    ●        + 0.12, 0.17, 0.25, 0.27
?                                      ●
                      ^ s0 0.025        ^ s1 0.055
```

**`s0 = 0.025`, `s1 = 0.055`.** Whites cluster at or below 0.025; colours start at 0.061
bar the ambiguous zone 0.028–0.042, which holds three whites and four colours (a leaf, a
pastel cloth, two sunset clouds). One white sits above `s1` and is treated as colour:
an Ektar cloth at 0.067, likely lit by coloured light — so 1 of the 12 in-range whites is
fully protected under these values. The user's intent sets the side: `s0` low, so the
operator cleans only the faint residual the white balance leaves, and a sunset-lit white
keeps its warmth.

**It holds across rolls.** Fitted on either roll alone, the band lands in the same zone;
on the held-out roll no `c` patch is fully pulled, and the only `w` treated as colour is
that same Ektar cloth. Portra contributes no in-range white —
the user found none on its frames — so it is tested on its colours only.

## The pair check

Mean C\* in the SDR output:

| | raw | + roll WB | + pull, no band | + band 0.025 → 0.055 | + band 0.020 → 0.035 |
|---|---|---|---|---|---|
| W (12) | 24.4 | 7.2 | 3.2 | 4.4 | 5.7 |
| C (17) | 30.6 | 17.0 | **7.2** | **16.1** | 16.8 |

Colours above `s1` keep their chroma — 100% from `s` 0.077 up, **96–99% just above
`s1`** (0.061–0.070), where some of a patch's pixels fall back into the ramp — while the
unbanded pull flattens them. Luminance holds (|ΔL\*| ≤ 0.17). **The whites below `s0` do
not clean identically**, which is the pair check's other half: four of the seven clean
less with the band than without it (1727 cloth 0.6 → 1.8 C\*, 1739 bird 1.2 → 2.8, 1735
bird 1.8 → 2.9, 1810 cloud 1.8 → 2.5). A patch is placed by its median, but its pixels
scatter across the band's edges, so some fall in the ramp. The gap is 0.7–1.6 C\* and was not visible by eye; it is a
property of a per-pixel band, and a band edge placed from patch medians
under-reaches by that scatter. By eye: the band's protection is visible on a bright saturated
surface (a rock) and not on skin, the two bands differ only slightly on the sunset clouds
(the wider one preferred), and **the operator's cleaning on top of the roll white balance
is not visible** — 7.2 → 4.4 C\* on the whites.

## Conclusions

1. **The operator needs a roll-level white balance upstream**, and is harmed by a
   per-frame one.
2. **Measure the band on `log10(max/min) / gamma`**, in the negative's density units —
   so the operator needs the roll's contrast.
3. **`s0 = 0.025`, `s1 = 0.055`**, provisional until
   `nf-calibration/anchor-comparison` chooses the anchor rule.
4. **Under a good roll white balance the operator's visible job is a guard rail, not the
   cleaning.** The white balance does that; the band is what keeps the pull from
   flattening colour where it does act.

## What this does not settle

- **The band's edges against pixel scatter.** Placed from patch medians, applied per
  pixel: both halves of the pair check miss by a little (see above). Carried to
  `path-to-white` as an open question.
- **Portra's white side**, and a roll with a blown frame for the leader guard.
- **The white-balance estimator.** It belongs to scene correction; this fit needed only
  a reasonable one.
- **The pull's strength and start**, held at 0.8 and one stop below white throughout.
  With the cleaning invisible on top of white balance, whether the operator earns a
  default-on place is `path-to-white`'s question.
- **A knee'd render**, which `path-to-white` asks to check the band under before shipping.
