# Where white lands on the display

> **Folded into `nf-calibration/span-roll-slope` on 2026-10-09.** The span slope maps the
> roll's white to a display target, so the target cannot be chosen apart from it; that
> task carries this one's evidence, open questions and review. HDR stays
> `white-rule-hdr`'s. This file is kept so existing references resolve.

## Goal

Choose a white target in display terms, so a frame's white is placed where the reviews
want it rather than at diffuse white.

## Design

Known (`docs/spike/poor-development.md`):

- On SDR, diffuse white renders at L\* 79, and every rule takes a frame's p97 as its
  white, so only the brightest 3 % can use L\* 79–100; that band is mostly empty.
- Reviews of steep placements preferred the white near L\* 90–93; the last one suggested
  90–92 (L\* 93 sat ~0.6 stop brighter than CCR in the midtones).
- After the target-0 exposure the roll's own white already lands near L\* 84–100; the
  low figures were the median frame's bright end. Pinning the roll's white at L\* 91 with
  exposure at the midtone flattened thin rolls (09-11 1.65 → 1.05), so the white target
  and the brightness target must be set together.
- `nf-display-stages/parametric-shoulder` reviewed a brighter white with mid-grey pinned
  and it lost, at the old −0.6 target and roll level only.

Open:

- Where the target lives (a placement, not a new curve) and how it combines with the
  level target and the envelope.
- What it means on HDR (`white-rule-hdr`).

## How to Verify

- A review at the default placement comparing today's white with one or two display
  targets, on a good, a thin and a poor roll.

## Dependencies

- [Envelope hybrid placement](envelope-hybrid-placement.md) — the placement the target
  is set within
- [Raise the roll's brightness target to 0](level-target-zero.md) — the other end of the
  same choice
