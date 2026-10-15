---
title: Per-frame placement
status: done
deps: [nf-calibration/span-roll-slope, nf-calibration/envelope-hybrid-placement]
tags: [new-flow]
summary: "`--placement frame`: each frame its own contrast from its span, its white at L\\* 93"
done: 2026-10-10
---
# Per-frame placement

> **From TASKS.md** (migrated 2026-10-14): `--placement frame`: each frame its own contrast from its span, its white at L\* 93; exposure limits ±1 EV (v13). **Done 2026-10-10**
>
> **Dependency note** (migrated 2026-10-14): filed 2026-10-09 (split from `envelope-hybrid-placement`): white- and midtone-pinned per-frame placements as choices, added to the placement choice the hybrid introduces

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

Settled (review, 2026-10-10; progress log): **one placement, white-pinned, `--placement
frame`, white at L\* 93**; midtone-pinned dropped. Every placement's exposure limits moved
to ±1 EV around the roll's (`pipeline_version` 13). A brightness-keyed white target was
considered and not adopted.

## How to Verify

- `measure-roll` on the archive rolls: every frame's slope and anchor, within the maximum.
- A review on a good and a poor roll.
- `docs/using-nc.md` describes the placement.

## Dependencies

- [The roll's slope from its span](span-roll-slope.md) — the white and dark targets
- [Envelope hybrid placement](envelope-hybrid-placement.md) — the placement choice this
  adds to
