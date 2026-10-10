# Per-frame placements

## Goal

Offer per-frame placement as a user's choice beside per-roll and the envelope hybrid:
each frame's contrast set by its own white and dark end, at its own anchor.

## Design

Known (`docs/spike/poor-development.md`, 2026-10-04/05):

- **Two variants were reviewed and both are worth offering**: white-pinned (the frame's
  white stays at the white target) and midtone-pinned (the frame's own midtone stays put).
- **A per-frame slope needs a per-frame anchor.** Pivoted at the roll's mid-grey it pushed
  low-key frames' highlights past white (9 % of pixels at white); pinned at the frame's
  white or its own midtone it did not.
- Frame span (display black off): per-roll 55.5 L\*, envelope hybrid 62.5–69.0, per-frame
  81–82, CCR 85.1. Steep, white-pinned per-frame placements matched CCR's tonal range on
  flat frames with the white at L\* 90–93.
- The global maximum slope applies wherever a slope is set, and binds most here: steep
  white-pinned slopes on flat frames (`hybrid-slope-bounds` sweeps it on these too).

Open:

- Whether both variants ship, and what midtone-pinned anchors on.
- **The white target.** Per-roll and the hybrid bring only the roll's brightest frame to
  it; white-pinned brings every frame there, low-key and night frames included. Default to
  `span-roll-slope`'s target; the review adds one lower-target arm on low-key frames, and
  only if that wins does per-frame get its own white.
- How the choice reaches the recipe and `measure-roll`: new values of the hybrid's
  placement enum, not a second field.

## How to Verify

- `measure-roll` on the archive rolls: every frame's slope and anchor, within the maximum.
- A review on a good and a poor roll, including the low-key white-target arm.
- `docs/using-nc.md` describes the placements.

## Dependencies

- [The roll's slope from its span](span-roll-slope.md) — the white and dark targets
- [Envelope hybrid placement](envelope-hybrid-placement.md) — the placement choice this
  adds to
