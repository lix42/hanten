---
title: Crop and rotate
status: todo
deps: []
summary: crop and rotate as flags and recipe keys; a crop also narrows the measurement region
---
# Crop and rotate

## Goal

Crop and rotate a frame, as CLI flags and recipe keys first, so the desktop app can use
them ([desktop app design](../../design/desktop-app.md) §2.4). A crop also narrows the
measurement region: it is intersected with the effective area, so changing a crop
re-measures that frame and can shift the roll's measurements.

## What is known

- Every knob is a flag and a recipe key (the project's knob rule); a per-frame value lives
  in `roll.frames`.
- The effective area (`pipeline/film_base.rs`) is the IR holder cut then a static inset;
  a crop would be a third cut.
- Not in scope: straighten (arbitrary angles), auto-crop.

## Open questions

1. Rotation in quarter turns only, plus flips?
2. Crop coordinates: before or after rotation, and in pixels or fractions.
3. Whether the roll's pooled measurements (white, envelope, content base) read the
   cropped area.
4. Whether a crop affects the memory model (`pipeline/memory.rs`).

## How to Verify

- A cropped, rotated frame's output has the expected size and orientation, and its report
  states the crop and the measurement region.
- An uncropped frame is byte-identical.
