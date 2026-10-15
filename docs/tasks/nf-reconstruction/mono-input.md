---
title: Mono input in the decode
status: todo
deps: [nf-reconstruction/mono-decode, io/gray-primary-decode]
tags: [new-flow]
summary: a gray scan, or a three-channel scan declared `--film-type silver`, decoded as one signal and carried as R = G = B
---
# Mono input in the decode

> **From TASKS.md** (migrated 2026-10-14): a gray scan, or a three-channel scan declared `--film-type silver`, decoded as one signal and carried as R = G = B; the per-channel knobs that could only tint are refused
>
> **Dependency note** (migrated 2026-10-14): supersedes `algo/bw-support`'s input half: a gray scan, or a three-channel scan declared `--film-type silver`, becomes one signal in the decode; the gray reader is needed only for the one-channel half

## Goal

A silver B&W negative carries one image signal, and the scanner records it either as
one gray channel or as three near-equal RGB channels. Both recordings must render
alike. Decode it as one signal, carry it as R = G = B, and refuse the knobs that could
only add a tint. The decision behind this task is
[`docs/design/mono.md`](../../design/mono.md).

## Design

What is decided (`mono.md`):

- **Which scans take this route.** Every one-channel scan, and a three-channel scan
  declared `--film-type silver` (`input.film_type`). Nothing infers the film type.
  This makes `film_type` gate something for the first time, so design-spec §9's
  "provenance only" wording changes with it. The IR holder detection stays measured.
- **Three channels.** Divide each channel by its own film base, then pool them into
  one signal. From that point on, the scan is a one-channel scan.
- **One curve.** Read only the red entries of `scale` and `offset`. The signal is
  copied into R, G and B before the 3×3, which keeps the channels within a couple of
  f32 ULP of each other.
- **Refused on mono input:** a white balance or midtone neutral that would apply, and a
  non-identity `channel_grade`. The rule reads the resolved correction, so gains a
  recipe keeps behind `roll.neutral_balance off` are not refused. A roll measurement
  over mono input writes no white-balance gains and no midtone line.
- **Output** stays three channels. A grayscale encode is a follow-up.

What is not known yet (wait on the HP5 scans, one recording of each kind):

- The pooling domain (transmission or density) and its weights. Match what the scanner
  does when it writes a one-channel file, found by comparing the two recordings of
  one frame.
- Whether the copied one-channel decode and the pooled three-channel decode agree on
  the same frame, and how closely.
- `pipeline::memory`'s figures for a one-channel scan widened to three f32 channels.
- Whether a mono run says so in the report, and where in the recipe the pooling
  weights live if they are a knob.
- Whether a stated green or blue `scale`/`offset` entry that differs from red is
  refused, and whether silver gets its own default `scale` (`mono.md`'s pending
  `[1, 1, 1]` item). The resolved default is the colour `[1, 0.84, 0.73]`, so a rule
  on the resolved value would refuse every default run.
- A recipe that already states `input.film_type: "silver"` on a three-channel scan
  renders colour today and will render mono: a new version row, refusing older
  recipes that state it, or an argument that neither is needed.
- How a one-channel scan's film base is carried: `calibration.film_base` and
  `--film-base` are R, G, B.
- Whether `measure-roll` needs the `--film-type` flag: `roll` takes it, but
  `measure-roll` sees `input.film_type` only through `--params`, and without it
  measures colour gains on a silver roll.
- Where a one-channel scan becomes three: at the reader, so `Scan.image` stays RGB and
  film base measures three equal channels, or as a one-channel `Scan` that film base
  and `measure-base` handle before the decode. Either way the route must still know
  the scan had one channel.

## How to Verify

- The one-channel and three-channel HP5 recordings of one frame both convert, and
  their outputs agree within a stated bound (derived numbers only).
- Every output pixel has R = G = B to within a couple of f32 ULP through the 3×3
  (more after lcms2; bound it on the output).
- Each refused knob fails with exit 2, and its message names the reason. Tests go
  through `merge` or the binary.
- Colour scans stay byte-identical: no golden or fingerprint moves for a scan that
  does not take this route (the stated-silver recipe above aside).
- `docs/using-nc.md` documents the route and the undeclared three-channel silver trap.

## Dependencies

- [Where black-and-white fits the new chain](mono-decode.md)
- [Decode a single-channel gray SilverFast scan](../io/gray-primary-decode.md) (only the one-channel half needs it)
