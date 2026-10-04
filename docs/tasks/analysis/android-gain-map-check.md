# Android gain-map check

> **Low priority** (user, 2026-10-04). Split out of
> [`analysis/viewer-interoperability`](viewer-interoperability.md), which covers the
> Apple readers, Chrome and an SDR-only reader; no Android device was available.

## Goal

Find out whether Android 15+ displays nc's gain-map JPEG as HDR. The file carries
ISO 21496-1 metadata only (no Ultra HDR v1 XMP) and a **three-channel** map; Android's
platform decoder is the non-Apple ISO reader, and the three-channel map is the risk.
This is the question `output/gain-map-dialect-activation` left when it closed.

## Design

Known:

- The file set and rubric are `viewer-interoperability`'s (`scripts/viewer-interop/`);
  this task runs the same rubric on Android and adds no second harness.
- `viewer-interoperability` runs Google's libultrahdr decoder (`ultrahdr_app`) on the
  gain-map files as a pre-check. It shows whether that parser reads the file, not
  whether Android's viewers select the HDR rendition; its result is the evidence this
  task starts from.

Open:

- Which device, or whether an emulator (API 35+) is acceptable evidence; an emulator's
  HDR display behaviour is not.

## How to Verify

The rubric is recorded in `docs/progress/analysis.md` for at least one Android 15+
device, with the device, OS version and viewer app, and every failure has a follow-up
task.

## Dependencies

- [Viewer interoperability](viewer-interoperability.md)
