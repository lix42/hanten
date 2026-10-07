# The envelope's anchor and the maximum slope

## Goal

Settle the two limits `envelope-hybrid-placement` ships with provisional values: where
the span-based per-roll slope anchors its exposure, and the global maximum slope.

## Design

Known (`docs/spike/poor-development.md`):

- With exposure at the midtone, the span-based roll slope puts the roll's whitest point at
  L\* 97.7, near clipping: placing both ends needs a slightly lower exposure, or a cap on
  the white.
- 3.5 is a placeholder from one review (max 3.5 with the white at L\* 93 beat max 3.0 at
  L\* 90 on low-contrast frames, saturation held). It must stay above the roll slopes
  (~1.75); inside the envelope it mainly binds very short-span frames.
- Whites rise on cast rolls once they are measured after colour correction
  (`nf-scene-correction/midtone-neutral`), which flattens those rolls' slopes.

Open:

- A fixed maximum, or one that depends on the frame: thinness (grain and scanner noise),
  how far its exposure moved, saturation near the gamut edge, the span itself; a soft cap
  or a hard one.
- Whether the dark end should be the darkest 1 % or 5 %.

## How to Verify

- A sweep of the maximum per frame on 09-18 and 09-29, flagging where shadows or colours
  visibly break, then a review of the chosen values against the placeholders.

## Dependencies

- [Envelope hybrid placement](envelope-hybrid-placement.md) — the placement whose limits
  these are
- [A midtone neutral measured per roll](../nf-scene-correction/midtone-neutral.md) — the
  whites these are measured from
