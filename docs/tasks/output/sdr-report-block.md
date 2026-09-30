# Machine-readable SDR contract in the report

> **Closed — moot (2026-09-30).** It was written against the `display-p3` /
> `compatibility` presets and the `SdrRenderMetadata` their renderer dropped; both went
> with `nf-core/default-flip`. Every destination's report now states its render as
> fields in `chain` (`nf-core/report-contract`): `fit_range` (operator, headroom, white
> point, display peak, display black), the fit-gamut map in `stages`, and `destination`.
> What follows is the plan as it stood.

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
