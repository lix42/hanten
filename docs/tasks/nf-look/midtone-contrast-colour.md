# Colour under a steeper midtone

## Goal

Decide whether saturation should follow the opt-in `--contrast 1.3`, once
[`midtone-contrast-toe`](../nf-display-stages/midtone-contrast-toe.md) holds its dark end,
or stay where it is.

## Design

Known:

- Colour stays at the roll's slope whatever `--contrast` is (`look.saturation` multiplies
  the colour's base slope; `contrast-on-luminance`; `docs/using-nc.md`, the look).

Open:

- Whether `--contrast 1.3` looks undersaturated next to the default, and if so whether
  colour should follow the frame's slope, a share of it, or stay at the roll's.

## How to Verify

- A review of renders at `--contrast 1.3` (with the toe), colour held at the roll's slope
  against colour following the contrast.

## Dependencies

- [More midtone contrast, the ends held by a toe and the
  shoulder](../nf-display-stages/midtone-contrast-toe.md) — the toe that holds
  `--contrast 1.3`, whose colour this decides
