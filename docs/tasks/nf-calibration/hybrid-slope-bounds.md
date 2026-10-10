# The maximum slope

## Goal

Settle the global maximum slope, which `span-roll-slope` introduces at a provisional
value, across every placement that sets a slope. The span slope's anchor and dark end moved to `span-roll-slope` (2026-10-09).

## Design

Known (`docs/spike/poor-development.md`):

- 3.5 is a placeholder from one review (max 3.5 with the white at L\* 93 beat max 3.0 at
  L\* 90 on low-contrast frames, saturation held). It must stay above the roll slopes
  (~1.75); inside the envelope it mainly binds very short-span frames.
- Whites rise on cast rolls once they are measured after colour correction
  (`nf-scene-correction/midtone-neutral`), which flattens those rolls' slopes.

Open:

- A fixed maximum, or one that depends on the frame: thinness (grain and scanner noise),
  how far its exposure moved, saturation near the gamut edge, the span itself; a soft cap
  or a hard one.

## How to Verify

- A sweep of the maximum per frame on 09-18 and 09-29, under the hybrid and the per-frame
  placements, flagging where shadows or colours
  visibly break, then a review of the chosen values against the placeholders.

## Dependencies

- [Envelope hybrid placement](envelope-hybrid-placement.md) — the default placement it
  limits
- [Per-frame placements](per-frame-placement.md) — where it binds most
- [A midtone neutral measured per roll](../nf-scene-correction/midtone-neutral.md) — the
  whites the slopes are measured from
