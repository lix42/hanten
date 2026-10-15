---
title: A plain SDR JPEG output
status: done
deps: [output/presets, output/sdr-display-rendering]
tags: [post-mvp]
done: 2026-10-11
---
# A plain SDR JPEG output

> **From TASKS.md** (migrated 2026-10-14): **done 2026-10-11.** `--range sdr --container jpeg` in Display P3, Adobe RGB and sRGB: the SDR TIFF's rendition as an 8-bit q95 4:4:4 JPEG, ICC only, through the gain-map base's encode path. Every destination row is now written, so the "not written yet" refusal is gone; telemetry `sdr_jpeg` needs the Worker deployed before a release

## Goal

Add an SDR JPEG output with no gain map: the same rendition the SDR TIFF holds, encoded
as an 8-bit JPEG for sharing. Before this task Hanten's only JPEG was the gain-map JPEG.

The id keeps its pre-destination name: on the current chain this is not a preset but
the `--range sdr --container jpeg` rows of `destination::ROWS`.

## Why now

On 2026-09-13 the user set the product shape: **SDR lossless is the default**
(`output/display-p3-default`), HDR lossless is supported, **SDR JPEG is supported**, and
HDR JPEG (the gain map) is good to have. The third did not exist.

## Decided (2026-10-11, with the user)

- **Rows, not presets**: three, Display P3, Adobe RGB and sRGB. Adobe RGB is added
  because a plain JPEG needs no decoder-verified gain-map base.
- **Encoding**: the gain-map base's, quality 95, 4:4:4, the ICC profile always embedded
  (sRGB too), through one shared encode path.
- **Telemetry**: `conversion.encoding` gains `sdr_jpeg`, widening the contract and the
  Worker in the same change with no schema or consent bump (the rule settled in
  `output/sdr-preset-followups`; the contract README states it).
- **No planned rows remain**, so the "not written yet" refusal (`Status::NotYet`) was
  removed rather than kept as dead code.
- **Memory**: shares `RunProfile::U16Tiff` by measurement.

## How to Verify

- `hanten convert --container jpeg -o out` writes `out.jpg`: an 8-bit JPEG whose encoder
  input is the SDR TIFF's byte for byte, with the TIFF's ICC profile and no MPF, XMP or
  ISO 21496-1 segment; repeated encodes are byte-identical.
- `hanten roll` derives `<stem>_positive.jpg`; a mismatched suffix is refused.
- Memory measured on two frame sizes; `nctool acceptance` covers the three rows;
  `docs/using-nc.md` updated by running the binary.

## Dependencies

- [Output presets and guidance](presets.md)
- [SDR display rendering](sdr-display-rendering.md)
