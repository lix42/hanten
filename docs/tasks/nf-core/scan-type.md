---
title: A scan type that ends at the decode
status: done
deps: [nf-core/buffer-strategy]
tags: [new-flow]
summary: split the scan (RGB, IR plane, `ir_verified`) from the post-decode image, so the types enforce that the IR plane stops at `fixed::decode`
done: 2026-10-11
---
# A scan type that ends at the decode

> **From TASKS.md** (migrated 2026-10-14): split the scan (RGB, IR plane, `ir_verified`) from the post-decode image, so the types enforce that the IR plane stops at `fixed::decode`. **Done 2026-10-11**: `Scan` (a `LinearImage` plus an optional `IrPlane`) is what the decoder returns and `fixed::decode` consumes; `LinearImage` is RGB only. Outputs byte-identical
>
> **Dependency note** (migrated 2026-10-14): filed 2026-10-09 from its review: the IR plane stops at the decode only because every post-decode `into_linear` passes `None`

## Goal

Make "the IR plane stops at the fixed decode" a property of the types, not of every
`into_linear` passing `None`. Today one `LinearImage` (RGB, `ir`, `ir_verified`) is
both what `io::decode` produces and what the chain hands the encoders, so after the
decode its `ir` field exists but must never be set.

## Design

- **The scan and the post-decode image are different things.** The scan carries RGB,
  the IR plane and its provenance (`ir_verified`); `film_base` reads all three, and
  `fixed::decode` consumes it. After the decode, an image has RGB only.
- Known consumers of the IR fields: `io::decode` (produces them), `film_base`
  (holder detection, `ir_verified` gate), `cli`'s holder warnings and notes, and the
  memory model's input shape. Everything from `FilmRgbImage` on, and the encoders,
  should never see them.
- **What goes with it:** the `None` passed by `FilmRgbImage::into_linear`,
  `AcesCgImage::into_linear` and `WorkingBuffer::into_linear`, and the guard in
  `hdr::EncodedHdrImage::from_linear_storage`, which cannot trigger now.
- Open: whether the post-decode image keeps the name `LinearImage` (and the scan gets
  a new one), or the other way round; how test producers that clone a scan
  (`branch_probe`, the chain tests) build one.

## How to Verify

- No type downstream of `fixed::decode` has an IR field; the decode's signature takes
  the scan type.
- Outputs byte-identical to the previous build on every destination (no pixel path
  changes).
- The four CI gates pass.

## Dependencies

- [Stage seams, buffers and the IR plane](buffer-strategy.md)
