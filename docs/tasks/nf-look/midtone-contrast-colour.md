# Colour under a steeper midtone

## Goal

Decide whether saturation should follow the steeper midtone that
[`midtone-contrast-toe`](../nf-display-stages/midtone-contrast-toe.md) brings, or stay
where it is.

## Design

Known:

- The placements hold colour at the roll's slope, so saturation does not follow a steeper
  midtone today (`look.saturation` multiplies the colour's base slope;
  `contrast-on-luminance`).

Open:

- Whether a steeper midtone looks undersaturated next to today's render, and if so whether
  colour should follow the frame's slope, a share of it, or stay at the roll's.

## How to Verify

- A review of the toe's renders with colour held at the roll's slope against colour
  following the steeper midtone.

## Dependencies

- [More midtone contrast, the ends held by a toe and the
  shoulder](../nf-display-stages/midtone-contrast-toe.md) — the steeper midtone whose
  colour this decides
