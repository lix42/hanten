# Envelope hybrid placement

## Goal

Make the envelope hybrid the default way a roll's frames are placed: each frame gets its
own contrast and exposure, but only inside the limits the roll's own placement sets. Keep
the per-roll and two per-frame placements as a user's choices.

## Design

Known (`docs/spike/poor-development.md`, 2026-10-04/05):

- **What it does.** Each frame's slope renders its scene span over at least R stops; its
  exposure keeps α of its offset from the roll's median level. Both stay inside the
  per-roll envelope: the roll's brightest and darkest points land where per-roll puts
  them. Default R 7, α 0.6, exposure within −1.5…+0.5 EV of the roll's.
- **The floor is the span-based per-roll slope** (roll white to the 10th percentile of
  the frames' darkest 1 %; 1.57–1.75 on ten rolls), and **one global maximum slope**
  applies wherever a slope is set (3.5, provisional — `hybrid-slope-bounds`).
- **Review** (whole rolls 09-29 and 09-18, colour and black-and-white): the hybrid is the
  default, "not too dramatic, not too flat"; per-roll, per-frame white-pinned and
  per-frame midtone-pinned are good alternatives to offer.
- **It generalises the small and thin lifts**: at α 0.6 night and thin frames render near
  today's thin lift.
- **It needs contrast on luminance**, now built (`nf-look/contrast-on-luminance`). Colour
  follows the base slope × `look::DEFAULT_SATURATION` (1.15), and a thin frame's slope
  never reaches it. A per-frame slope stays out of colour only if it arrives the way
  `roll.thin_slope` does. A per-frame `white_stops` sets the colour's base too.

Open:

- How the placement choice and R and α reach the recipe and `measure-roll`; their classes
  (correction / guard / preference).
- What becomes of the small and thin lifts and their switches.
- Display black's role once contrast is set this way, and the shadow toe steep slopes need
  (1719 loses shadow detail).

## How to Verify

- `measure-roll` on the archive rolls: every frame's slope and exposure inside the
  envelope, and the roll's brightest and darkest points equal to per-roll's.
- A review confirming the implemented hybrid matches the spike's `../temp/a4-review/`.
- A drift-gate row; `docs/using-nc.md` describes the placements.

## Dependencies

- [Raise the roll's brightness target to 0](level-target-zero.md) — the exposure the
  hybrid starts from
- [Separate taste from quality](taste-vs-quality.md) — the lifts this generalises, and
  the switch shape
- [Contrast on luminance, saturation its own
  setting](../nf-look/contrast-on-luminance.md) — so a steeper frame is not also a more
  saturated one
