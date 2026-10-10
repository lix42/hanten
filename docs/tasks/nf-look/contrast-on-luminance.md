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

Decided (user, 2026-10-09):

- **Saturation scales log channel ratios** (`(x_c / Y)^s`, luminance restored), not a
  linear chroma scale about luminance. It never makes a channel negative, and highlight
  desaturation's band keeps its meaning by dividing by the saturation slope.
- **`look.saturation` is a multiplier, default 1, as `look.contrast` is.** The saturation
  slope is the base slope (never a thin frame's) × the rendering's saturation × the
  knob. The `default` rendering's is **1.15** (`look::DEFAULT_SATURATION`), from the
  review below; `direct` pins 1.
- **Recipes are not migrated**: a `pipeline_version` bump (10) and a drift row. A recipe
  stating `look.contrast` now renders its colour at the default saturation.
- **Review** (`../temp/saturation-review/`; held / ×1.15 / ×1.3 / CCR, 11 frames on 09-18,
  09-20 and 09-29, today's per-roll placement, display black and highlight desaturation
  off): ×1.15 chosen. Mean frame C\* 12.3 / 14.1 / 15.9, CCR 14.6. Marked whites'
  leftover cast 7.7 / 8.8 / 10.0. At most 2.5 % of a frame outside sRGB.

## How to Verify

- A neutral renders as before at every contrast; a coloured patch's chroma no longer moves
  with contrast.
- A saturation review (held / ×1.15 / ×1.3 / CCR) on frames with colour patches.
- A drift-gate row; `docs/using-nc.md` states the setting.

## Dependencies

- [What `look.contrast` means](contrast-definition.md) — the knob this changes
- [A midtone neutral measured per roll](../nf-scene-correction/midtone-neutral.md) — the
  corrected colour the amount is judged on
