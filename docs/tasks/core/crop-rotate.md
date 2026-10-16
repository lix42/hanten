---
title: Crop and rotate
status: done
deps: []
summary: crop and rotate as flags and recipe keys; a crop also narrows the measurement region
done: 2026-10-16
---
# Crop and rotate

## Goal

Crop and rotate a frame, as CLI flags and recipe keys first, so the desktop app can use
them ([desktop app design](../../design/desktop-app.md) §2.4). A crop also narrows the
measurement region: it is intersected with the effective area, so changing a crop
re-measures that frame and can shift the roll's measurements.

## What is known

- Every knob is a flag and a recipe key (the project's knob rule). A frame's own crop goes
  in the shared recipe's `geometry.frames` table, keyed like `roll.frames`,
  so `measure-roll` (which never reads a `roll --frames` manifest) measures it too.
- The effective area (`pipeline/film_base.rs`) is the IR holder cut, then a static inset,
  then the crop as a third cut.
- Not in scope: straighten (arbitrary angles), auto-crop. Seeding a crop from the
  measured holder belongs to the desktop app.
- "The image is never cropped" (design-spec §9) and "It never crops the image"
  (`docs/using-nc.md`) change with this task.

## Decided

The answers, and why, are in `docs/progress/core.md` `## crop-rotate`.

1. Quarter turns plus flips; the flip comes before the turn.
2. Pixels in the scan's own orientation, before any turn (the `--base-region`
   convention).
3. No batching: a crop is part of the recipe, so measuring with it measures each frame
   over its crop, and `measure-roll --out` carries it to `roll`.
4. No new term in the memory model: the crop compacts in place, the turn permutes in
   place.

## How to Verify

- A cropped, rotated frame's output has the expected size and orientation, and its report
  states the crop and the measurement region.
- An uncropped frame is byte-identical.
