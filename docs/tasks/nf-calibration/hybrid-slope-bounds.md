# The maximum slope

**Done:** 2026-10-11 — the limit guard and the measurements; the maximum's value moved to
[`short-span-protection`](short-span-protection.md), the tails to
`nf-display-stages/midtone-contrast-toe` (`docs/progress/nf-calibration.md`).

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
- The hybrid's own bounds, from `envelope-hybrid-placement`'s review (2026-10-10; its
  progress entries hold the archive numbers). With display black now off, the hybrid
  alone places each frame's ends:
  - The ±EV limit (±1 since `per-frame-placement`, was −1.5 / +0.5) applies after the
    envelope clamp, so it can push a frame out of the envelope: at −1.5 / +0.5 the dark
    ends of 17 limit-held archive frames fell below the roll's (09-29 −5.68 against −4.42
    look stops); a high-key short-span frame could push its white above the top. Lower the
    slope when the limit binds, or clamp to the envelope last? The −1 end is unreviewed.
- `--placement frame`'s white (L\* 93): in its review, L\* 91 won on 7 high-key frames
  (`per-frame-placement`'s progress entry names them), though a brightness rule lost
  overall. Revisit if high-key frames keep looking too bright there.
  - The kept exposure, `e0 − (1−α)(L + e0)`, measures a frame's offset from a level target
    of 0, not from the roll's level; they differ when the roll's exposure was clamped
    (`EXPOSURE_BOUND_EV`), or typed, and it silently depends on `LEVEL_TARGET_STOPS` = 0.
  - The envelope's top is the highest unclamped white at the roll's slope; when that frame
    is clamped, per-roll renders it lower (at the cap's slope), so hybrid whites can sit
    above per-roll's brightest point. Not seen on the archive.

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
