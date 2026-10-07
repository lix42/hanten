# Rolls from a poor development: colour cast and tone placement

What Hanten should do with rolls it handles badly today: 2026-09-29-Ektar100 and
2026-09-28-Portra400-dark, both developed in an exhausted developer. They come out violet
in the bright parts, flat, and dark next to SilverFast's CCR export. The spike ran
2026-10-02 to 2026-10-06 on ten archive rolls, with CCR exports for 09-18, 09-20 and 09-29
as the reference. Scripts, sets, notes and raw numbers are in `../temp/roll-neutral-spike/`
(its `TODO.md` is the full trail) and the review sets beside it (`../temp/a4-review/`,
`b2-review/` … `b7-review/`).

It produced one filed task already (`nf-calibration/level-target-zero`) and five more:
`nf-scene-correction/midtone-neutral`, `nf-calibration/envelope-hybrid-placement`,
`nf-calibration/hybrid-slope-bounds`, `nf-calibration/display-white` and
`nf-look/contrast-on-luminance`.

## Method

- **One layer at a time, and say which**: density after the decode, the look stage, or
  final L\*/Lab — never compared across layers.
- **CCR is the reference** for colour and exposure. NLP's look is not: its midtones sit
  9–16 L\* above CCR's.
- **The default removes only the cast reconstruction leaves.** The scene's light (an
  orange interior, a sunset, blue shade) and the stock's character stay; a more neutral
  look is opt-in.
- **Review rounds isolate one question.** Colour rounds hold the tone placement fixed
  (computed once from the uncorrected measurement), turn display black and highlight
  desaturation off (both hide what the change under test does), and lock saturation
  within each arm — each arm's own chromaticity at a fixed slope, its luminance from the
  real slope — then apply the same ×1.15 to every arm. Tone rounds add a black-and-white
  copy of every cell. At most four arms.
- **Code under test** is on the branch `spike/midtone-guard` (commit `b864b63`, on
  `b2e99f4`; pushed, never to be merged): two environment hooks, `NC_SPIKE_MIDTONE_DECODE`
  in the decode and `NC_SPIKE_MIDTONE_GRADE` in scene correction. With neither set it is
  bit-identical to `b2e99f4`.
- **The trail** — scripts, numbers, the round-by-round notes and `TODO.md`, without the
  renders — is also copied to the user's Google Drive, `My Drive/temp/roll-neutral-spike/`.

## Tone placement

- **Target 0.** Putting the median frame at mid-grey instead of 0.6 stop under it matches
  CCR's midtones on its brighter frames and won review on a good and a poor roll. Filed as
  `level-target-zero`.
- **Contrast and saturation are coupled today, and should not be.** The look's contrast
  is a power on each channel, so a steeper slope multiplies chroma (2.0 → 3.0 ≈ 1.5×).
  Rendered in colour, a steep slope lost on saturated frames; with saturation held at the
  gentler slope's level, the steep slope won on 10 of 12 low-contrast frames. Contrast
  belongs on luminance, saturation in its own setting.
- **Diffuse white renders at L\* 79 on SDR**, and every rule here takes a frame's p97 as
  its white, so only the brightest 3 % can use L\* 79–100. On 209 frames the brightest
  0.1 % reaches L\* 87.5 on 97 frames and 93 on 29: that band is mostly empty. Steep,
  white-pinned per-frame placements matched CCR's tonal range on flat frames with the
  white at L\* 90–93.
- **Per-frame placement needs a per-frame anchor.** A per-frame slope pivoted at the
  roll's mid-grey pushes low-key frames' highlights past white (9 % of pixels at white);
  pinned at the frame's white or its own midtone it does not.
- **The envelope hybrid is the default candidate.** Each frame gets a slope that renders
  its scene span over at least R = 7 stops, and an exposure keeping α = 0.6 of its offset
  from the roll's median level — all inside the per-roll envelope, so the roll's brightest
  and darkest points stay where per-roll puts them. Frame span (display black off):
  per-roll 55.5 L\*, envelope hybrid 62.5–69.0, per-frame 81–82, CCR 85.1. Whole-roll
  review on 09-29 and 09-18: "not too dramatic, not too flat… a real good balance". The
  per-roll and two per-frame placements (white-pinned, midtone-pinned) are good choices to
  offer, not losers. α 0.6 also brings night and thin frames back near today's thin lift,
  which α 1.0 removed.
- **A per-roll slope from the roll's span** (roll white to the 10th percentile of the
  frames' darkest 1 %) is far more consistent than today's white-to-midtone slope: 1.57–1.75
  on all ten rolls against 1.05–1.57, thin rolls rising most. It is the envelope's floor.
  Catch: with exposure at the midtone, the roll's whitest point lands at L\* 97.7.
- **Display black is still needed**: a span slope sets the distance between the ends, not
  where the dark end sits. Steep slopes lose shadow detail (1719), which display black
  cannot return — it only pushes the dark end down.

## Colour cast

The cast on 09-29 grows away from the roll's p99 density, where today's roll white
balance is exact: a slope error per channel plus an offset on blue.

| method | how | result |
|---|---|---|
| two-point slope | per-channel scale pivoted at the base, neutral at the roll's p99 | helps 8 of 10 rolls, partially: 09-29 21.4 → 17.4 against CCR 5.8. Superseded. |
| midtone neutral, plain line | per band of brightness, each frame votes the densest cluster of its colour ratios; the roll's value is the median vote; a weighted line through the bands | large gains on poor rolls; extrapolated past its data it turned 09-20's sky pink |
| + guard | flat outside the voted bands | fixes extrapolation; 09-20's top band still off by 66 milli-density, because the votes curve |
| white-anchored | line through neutral at the roll's white | right on bright frames, costs midtones elsewhere |
| **joined** | plain line up to 1 stop below the roll's white, fading to zero at it, flat above | **no bad frame on any reviewed roll; best most often** |

Neutral-patch cast (median \|a\*, b\*\|, before the ×1.15), today's WB / two-point / joined:
09-29 21.4 / 17.9 / 3.4 (CCR 5.8); 09-20 15.5 / 13.1 / 8.8; on well-developed rolls 09-18
6.7 / 5.9 / 8.3 and 09-14 6.3 / 6.3 / 7.5. On those two the eye disagreed with the patches:
joined was best most of the time and never bad.

- **Decode form vs grade form.** The same line can act in the decode (per-layer scale and
  offset) or in scene correction (per pixel against scene stops, luminance restored).
  Review preferred the grade form (33 better / 17 tie / 14 worse), not uniformly; the
  candidate is the scene-correction step.
- **When it turns on.** Detecting *poor development* failed: frame-white spread stopped
  sorting rolls once the line was joined, and the best signal (drift ÷ vote scatter) did
  not separate cleanly. Since joined does no visible harm on good rolls, the switch
  becomes a **data floor**: on unless the roll has fewer than 10 frames or too few voted
  bands (the three- and four-frame rolls lost clearly).
- **What it cannot tell from cast**: warm light that fills a frame's midtones (09-20
  1883, a sunset-lit cloud underside) — today's WB rendered it best. A roll whose frames
  share one dominant colour (beach sand, sky, foliage) breaks the voting assumption that
  the most common colour at each brightness is grey; none of the ten rolls tests it.
- **The decode's Ektar constants may carry a residual.** The line's average correction is
  44 (×1000 log ratio) on well-developed 09-18 Gold, 300–400 on the poor rolls, and 270 on
  well-developed 09-14 Ektar, where applying it changed nothing visible. A per-stock
  error belongs in the decode, not a per-roll line — a question for `offset-question`.

## Measurement order

Each frame's white is the p97 of the brightest channel, measured on film RGB **before**
white balance. So any colour correction, right or wrong, moves the roll's white, hence the
roll slope and every frame's placement: the decode form raised 09-20's roll white 1.68 →
1.92. The agreed order is **two passes over the same samples, no loop**: (1) colour —
per-channel p99 pooled over the roll gives the white balance and the midtone line;
(2) tone — each frame's white measured on the corrected samples. No loop is needed because
the slope is channel-equal and pivoted at mid-grey, so a neutral white stays neutral.
Whites rise on cast rolls and their slope flattens; the white rule's p97, cap +2.0 and
floor +1.5 were tuned on pre-correction whites.

## As reviewed

The exact definitions behind the approved renders, so an implementation can be checked
against them. Units: *scene stops* are log2 of ACEScg luminance (`ACESCG` luma) over 0.18
at exposure 0; *look stops* are the same after the slope.

**Midtone neutral, joined** (`../temp/roll-neutral-spike/scripts/detect_probe.py`, `b2_probe.py`):

- Samples: every picture frame's effective area at the fixed decode's output, in ACEScg,
  times the roll white balance (`measure-roll`'s gains).
- Bands: 0.5 scene stop wide from −3.0 to +2.5. Per frame and band with at least 300
  pixels, the vote is the centre of the densest cluster of `(log2 r/g, log2 b/g)`: start
  at the median, then up to 5 times take the median of the pixels within 0.15 of the
  centre, stopping when fewer than 75 remain.
- A band counts with votes from at least 3 frames; its value is the median vote. Per
  channel a line `α·s + β` is fitted through the counted bands' centres by least squares
  weighted by √(frames voting).
- `s_w`: the scene stop of the pooled per-channel p99 (20 000 samples per frame).
- Per pixel, `s` = scene stop of the pixel times the white balance; `fade` = 1 up to
  `s_w − 1`, falling linearly to 0 at `s_w`, 0 above; `s` clamped to the counted bands'
  centres and to at most `s_w − 1`. Red and blue are multiplied by `2^−(α·s + β)·fade`,
  then all three channels are scaled to restore the pixel's luminance. Applied before the
  white balance gains.

**Envelope hybrid** (`b1_probe.py`, `plans`): per frame, `w` its white (scene
stops, `measure-roll`), `L` its level, `d` the p1 of its scene stops; `e0` the roll
exposure at target 0.

- Span roll slope `k_sr = (4.049 + 5) / (w_roll − d_roll)`, where 4.049 and −5 are the
  white and dark targets in look stops (white ≈ L\* 91) and `d_roll` is the 10th
  percentile of the frames' `d`; held within [1.237, 3.5] (1.237 = the white cap's
  minimum slope, 3.5 the global maximum).
- Envelope: `env_w = max k_sr·(w + e0)`, `env_d = min k_sr·(d + e0)` over the roll's
  frames.
- Frame slope `k = max(min(max(k_sr, 7 / (w − d)), (env_w − env_d) / (w − d), 3.5),
  min(k_sr, 1.237))` — R = 7.
- Frame exposure `e0 − 0.4·(L + e0)` — α = 0.6 — clamped to `[env_d / k − d,
  env_w / k − w]` (their midpoint if empty), then to `[e0 − 1.5, e0 + 0.5]`.
- Rendered through a frame entry's `thin_slope` / `thin_exposure`, display black off.

**Saturation lock** (colour rounds): each arm rendered twice, at `(k_sr, e0)` and at its
hybrid placement; the shown pixel is the first's linear Display P3 RGB scaled by the ratio
of the second's luminance to the first's. Patches are measured there; the viewing JPEG
adds `y + 1.15·(rgb − y)` around each pixel's luminance.

## Lessons for the next spike

- **Patch medians over-read small differences.** Twice they ranked arms against what the
  eye saw (09-29 3.4 → 6.4 with no visible change; good rolls' joined "worst" by 1–2.5).
  Close calls need ground truth (a ColorChecker), not a patch median.
- **A white patch's cast is not what the eye sees**: halving it left the visible violet in
  coloured areas (sky, midtones). Measure coloured areas too.
- **A regression of one producer's levels on another's is diluted by noise**: "CCR keeps
  78 % of the frame-to-frame differences" was that artefact; measured directly CCR is not
  more levelled, just brighter.
- **A control that fixes placement must be called a control.** The two-pass order is the
  design; holding placement fixed only kept colour rounds about colour.

## Not settled

- **The midtone line's fit range** (every voted band, or only those below the fade) and
  **the fade width** (1 stop, untested). Fitting below the fade made 09-20 visibly warmer,
  neither better nor worse. Waits for ColorChecker frames.
- **The beach case**: candidates are a second pass on pixels near neutral after the first
  correction, a guard against strong disagreement with the white, and weighting bands by
  how well frames agree (which helps mixed scenes, not the beach).
- **1883's warm light**: a tint gate or a user turn-down.
- **The envelope's anchor and the maximum slope.** 3.5 is a placeholder; whether it is a
  fixed value or depends on the frame (thinness, how far its exposure moved, saturation).
- **Where white lands in display terms** (L\* 90–92 by the reviews) and how that target
  combines with the brightness target.
- **The amount of saturation** once contrast is on luminance; Hanten sits far below CCR
  (09-18 colour patches C\* 7.7 vs 26.3).
- **Highlight desaturation's role** as a final cleanup on top of the midtone line; kept
  off in every colour round.
- **What CCR does per frame** (a black/white stretch, a channel balance?) — not measured.
