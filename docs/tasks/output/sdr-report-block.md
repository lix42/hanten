---
title: Machine-readable SDR contract in the report
status: closed
deps: [output/presets]
tags: [post-mvp]
---
# Machine-readable SDR contract in the report

> **From TASKS.md** (migrated 2026-10-14): **closed—no block** (2026-10-11): the SDR TIFF and the gain-map base need no encoder block. An SDR encoding states no absolute luminance, so its ICC profile and `chain.destination` define it, and the gain map's values are in `chain.gain_map`; recorded in design-spec §5 and `docs/using-nc.md`
>
> **Dependency note** (migrated 2026-10-14): **closed—no block** 2026-10-11; the dep below is decision history

> **Closed — no block (2026-10-11).** The HDR blocks exist for what an ICC profile cannot
> carry: reference white, peak, headroom and content light in cd/m². An SDR encoding
> states none of those. Its bit depth, sample format, curve and primaries are fixed by
> `chain.destination`, and the embedded profile states them. Clipping is in `loss`,
> BigTIFF promotion is a warning, and the render is in `chain`. For the gain-map JPEG,
> `chain.gain_map` carries the gain range and the base's fit range, and the file's
> headroom equals `chain.fit_range.display_peak`. A block would only repeat fixed
> values. Recorded in design-spec §5 and `docs/using-nc.md`'s HDR destinations
> section.

> **Re-scoped (2026-09-30).** The presets and the `SdrRenderMetadata` below went with
> `nf-core/default-flip`, and the render half is done: every destination's `chain`
> states its operator, headroom, peak, gamut map and destination
> (`nf-core/report-contract`). What is left is the **encoder** half. The HDR TIFFs
> each report a block of what was written (`pixel_contract`,
> `bits_per_sample`, `icc_bytes`, …); the SDR TIFF and the gain-map base report none.
> Decide whether they need one — the HDR blocks exist because an ICC profile cannot
> carry luminance, which an SDR profile does not need to — and add it or close this.
> The plan below is as it stood.

## Goal

Make the SDR presets' render contract machine-readable in the JSON report, the way
the HDR TIFF presets already are. Today `display-p3` and `compatibility` describe
their render only as prose in `output_render.content`, while `hdr-linear-tiff` emits
`hdr_linear_tiff` and the coded presets emit `hdr_coded_tiff`.

## What is known

- Split out of `output/sdr-preset-followups` on 2026-09-13.
- `stages::render_sdr_preset` drops `SdrRenderMetadata` as `_metadata`. That struct
  already carries the field set: reference white, shoulder start, tone curve, gamut
  mapping, linear domain. None of it reaches the report.
- `sdr::RenderedSdr::metadata()` carries a `#[allow(dead_code)]` naming this work as
  its consumer. Removing that allowance is the mechanical definition of done.
- The `hdr_coded_tiff` block is the shape to follow. The tone field must reflect the
  *resolved* `display_tone` (`shoulder` / `none` / `reinhard` with its headroom), the
  same rule CLAUDE.md records for report prose: prose that names an operation is a
  claim about the run.

## How to Verify

- `hanten convert --output-preset display-p3 --report json` carries the SDR contract as
  fields, and the same for `compatibility`; the block agrees with the resolved
  `print.display_tone`.
- The `#[allow(dead_code)]` on `RenderedSdr::metadata()` is gone.
- `docs/using-nc.md` documents the block, verified by running the binary.

## Dependencies

- [Output presets and guidance](presets.md)
