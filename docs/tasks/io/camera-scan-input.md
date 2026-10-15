---
title: Read camera scans
status: todo
deps: []
summary: accept camera-scanned negatives (raw/DNG first); a release gate for the desktop app
---
# Read camera scans

## Goal

Convert negatives scanned with a digital camera, not only SilverFast HDR/HDRi scans.
Camera scanners are part of the desktop app's audience, so this is a **release gate**
for the app ([desktop app design](../../design/desktop-app.md) §2.12).

## What is known

- Today only SilverFast HDR/HDRi is read. `--input-meaning colorimetric` is recognized
  and refused, and camera RAW / DNG is out of scope in the design spec (§2, roadmap
  item 4).
- A camera file has no IR plane, so holder detection falls back to the static inset
  alone, and IR dust removal (when it exists) cannot apply.
- The decode was tuned against scanner data (`docs/design-update.md` Part 3); how far it
  holds for a camera's sensor is unknown.
- No camera-scanned roll is in `nc-assets` yet.

## Open questions

1. Which formats first: raw (Bayer/X-Trans) through a decoder, DNG, or a linear TIFF
   from a raw converter?
2. What the samples measure (a camera's native space vs a colorimetric one) and how
   they reach the density path.
3. Whether the content base and roll measurements hold on camera scans
   (`film-base/content-base-rebate`).
4. What `inspect`, `measure-base` and `measure-roll` report for a camera file.

## How to Verify

- A real camera-scanned roll measures and converts to the default destination, with a
  report that states how the input was read.
- SilverFast scans are byte-identical through decode and convert.
