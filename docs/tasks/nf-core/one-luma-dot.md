---
title: One luminance dot product
status: done
deps: [nf-look/path-to-white]
tags: [new-flow]
done: 2026-09-24
---
# One luminance dot product

> **From TASKS.md** (migrated 2026-10-14): **done 2026-09-24.** `colorimetry::dot` is the one f32 copy; the four private ones are gone and the look no longer imports fit range's. No pixel moved, no golden edited
>
> **Dependency note** (migrated 2026-10-14): `dot` is copied in four stages, and the look imports fit range's. Done 2026-09-24: one copy in `pipeline::colorimetry`

## Goal

One shared copy of the 3-vector `dot` that takes luminance against a pinned luma
vector, used by every stage that needs it.

## Why it exists

`dot(a, b)` is copied privately in `pipeline/{sdr,hdr,gain_map}.rs` and in
`pipeline/fit_range.rs`, whose copy became `pub(in crate::pipeline)` so the look
(`nf-look/path-to-white`) could import it — coupling a stage to the one after it for
a utility.

## Known

- A natural home is beside the luma vectors in `pipeline/colorimetry`, or next to the
  pixel payload (`pixels` / `working_image`); pick whichever the callers read best from.
- It must not move a pixel: the shared copy keeps the same f32 operation order, and
  every golden (`stages::golden`, `chain_golden`) stays bit-identical.

## Open questions

- Whether the current-chain copies (`sdr`, `hdr`, `gain_map`) are worth touching
  before `nf-retire` removes them, or whether only their imports change.

## How to Verify

- One definition of `dot` in `src/`; all four gates green with no golden edited.

## Dependencies

- [Highlight desaturation](../nf-look/path-to-white.md) — its look imports
  `fit_range::dot`.
