# Highlight desaturation under the white rule

Produced by `nf-look/desaturation-band-refit`, 2026-09-30, against the shipped chain
(`pipeline_version` 8, commit `c2c3b58`): the roll white, white balance and exposure from
`hanten measure-roll`, display black at its default, SDR Display P3. Scripts, recipes and
raw numbers in `../temp/band-refit/` (uncommitted — the frames are the user's photographs).

## The answer

**Nothing moves.** `look.highlight_desaturation` keeps strength `0.8`, start one stop below
diffuse white and band `0.015 → 0.025`. The pair check passes under the rule and display
black, and in review the user saw a difference on one frame (1810, default better than
off) and none between the default and a wider band.

- **Contrast does not move a pixel against the band.** `s = log10(max/min) / (linearization
  · slope)`, and contrast is a per-channel power after white balance, so the divisor takes
  out exactly what the power puts in: every marked white's `s` was identical at slope ×0.75,
  ×1 and ×1.5. What contrast changes is how much chroma a white *renders* with (near-neutral
  whites 8.0 → 15.9 C\* at ×1.5) and how many pixels reach the brightness ramp. The operator
  removes the same share at every contrast, so whites' chroma rising with contrast is not
  the band's to take back.
- **The band is only as good as the roll's white balance.** Measured over the trimmed roll
  (8 of 32 frames), Ektar 09-14's gains came out ~10% off the full roll's, which put skin,
  rock and a ski at `s` ≈ 0.021, inside the band, losing 26–35% of their chroma. Under the
  full roll's gains the same patches sit at 0.049–0.056, untouched. A 10% gain error moves
  `s` by more than the band is wide: measure a roll whole.
- **No band separates better.** The one colour near the band is a pastel cloth (1727, `s`
  0.023); 09-20 Portra's bright whites sit just above it (0.025–0.027), so reaching them
  costs the pastel. The next colour is at 0.049, and whites under coloured light (sunset,
  interior) start near 0.044 — which the roll white balance's intent says to keep.

## Method

Patches: the asset manifest's marked rectangles (trimmed `../nc-assets`), deduplicated —
**20 near-neutral whites** (`s` < 0.035), 24 whites under coloured light, **11 colours**
(7 on Ektar 09-14) above 1.5 stops below white, on nine rolls. No bright Portra white is
marked outside 09-20.

Recipes: `measure-roll` over the **full archive roll** (`--unexposed`, `--leader`). The
placement simulated the look on cached film-master pixels — roll gains × `2^exposure`,
the contrast power about 0.18 at the frame's slope, then a copy of `pipeline::look`'s pull
— and was checked against the binary (HDR linear BT.2020, fit range and black off) to
within 1 C\* and 0.005 Y on three frames. The pair check ran the binary.

## Numbers

Placement, mean C\* of near-neutral whites after the pull (7.95 before) and the worst
colour's chroma kept:

| band | strength | whites C\* | worst colour |
|---|---|---|---|
| `0.015 → 0.025` (default) | 0.8 | 5.60 | 0.85 (pastel) |
| `0.015 → 0.025` | 1.0 | 5.01 | 0.81 |
| `0.015 → 0.020` | 0.8 | 6.23 | 0.96 |
| `0.015 → 0.030` | 0.8 | 5.12 | 0.64 |
| `0.020 → 0.030` | 0.8 | 4.53 | 0.47 |

A start at −1.5 stops changed whites by ≤ 0.1 C\*. Whites at `s` ≤ 0.017 keep 21–47% of
their chroma; those on the ramp (0.019–0.023) 53–98%; those above `s1` all of it.

Pair check through the binary, 17 frames, mean patch C\* (CIELAB, encoded Display P3):

| strength | whites (20) | colours (11) |
|---|---|---|
| 0 | 6.28 | 19.82 |
| 0.5 | 5.21 | 19.76 |
| 0.8 | 4.55 | 19.74 |
| 1.0 | 4.12 | 19.72 |

Luminance held: |ΔL\*| ≤ 0.01 from 0 to 0.8. Every colour keeps 100% but the pastel
(6.0 → 5.1).

Review (17 frames, off / default / `0.015 → 0.030`): a visible difference on 1810 only —
its cloud 7.8 → 4.1 → 3.2 C\* — where the default beat off and the wider band looked the
same as the default.

## Left for others

- **09-20 Portra's bright whites keep C\* 10.8–13.1**, clustered at `s` 0.025–0.027: a
  roll-constant residual the gains left, so it is the white balance's or
  `look.channel_grade`'s, not this operator's.
- **The colour side is thin and Ektar-heavy**; a re-placement would want more marked
  colours near the band, on other stocks.
