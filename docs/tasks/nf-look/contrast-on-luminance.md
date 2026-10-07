# Contrast on luminance, saturation its own setting

## Goal

Make the look's contrast change luminance only, and give saturation its own setting, so
a steeper slope stops adding colour as a side effect.

## Design

Known (`docs/spike/poor-development.md`, 2026-10-04):

- Today's contrast is a power on each ACEScg channel, so a steeper slope multiplies chroma
  (2.0 → 3.0 ≈ 1.5×). In colour a steep slope lost on saturated frames; with saturation
  held at the gentler slope's level it won on 10 of 12 frames. The structure is settled:
  contrast on luminance, saturation separate.
- Holding chroma at brighter levels pushes bright saturated colour out of the display
  gamut (up to 21 % of a frame in sRGB in the test) — `fit_gamut`'s job, worth watching.
- Hanten renders far less saturated than CCR (09-18 colour patches C\* 7.7 vs 26.3).
  Reviewers asked for a little more than the held level; the colour rounds used ×1.15.

Open:

- The default amount of saturation: a review on cast-corrected colour, since a residual
  cast is chroma any saturation setting amplifies.
- How the change is spelled for recipes that set `look.contrast` today.

## How to Verify

- A neutral renders as before at every contrast; a coloured patch's chroma no longer moves
  with contrast.
- A saturation review (held / ×1.15 / ×1.3 / CCR) on frames with colour patches.
- A drift-gate row; `docs/using-nc.md` states the setting.

## Dependencies

- [What `look.contrast` means](contrast-definition.md) — the knob this changes
- [A midtone neutral measured per roll](../nf-scene-correction/midtone-neutral.md) — the
  corrected colour the amount is judged on
