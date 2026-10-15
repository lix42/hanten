---
title: Decode a single-channel gray SilverFast scan
status: todo
deps: [io/silverfast-decode]
related: [nf-reconstruction/mono-input, film-base/ir-usability-detection]
priority: low
tags: [post-mvp]
summary: accept a 16-bit grayscale primary (IR page unchanged); nc refuses these HP5 scans today
---
# Decode a single-channel gray SilverFast scan

> **From TASKS.md** (migrated 2026-10-14): **low priority** (user, 2026-09-29). Accept a 16-bit **grayscale primary** (IR page unchanged). nc refuses these outright today: seven real Ilford HP5 frames fail with `found Gray(16)`, each carrying a marker-verified IR page. Neither existing task owns it — `io/silverfast-decode` required `Gray(16)` only for the IR plane beside an RGB IFD0, and `algo/bw-support` explicitly excludes input-format work. Blocks the one-channel half of `nf-reconstruction/mono-input`; it becomes three equal channels inside ([`design/mono.md`](design/mono.md))
>
> **Dependency note** (migrated 2026-10-14): **low priority** accept a 16-bit **grayscale primary** (IR page unchanged). Neither existing task owns it: `io/silverfast-decode` required `Gray(16)` only for the IR plane beside an RGB IFD0, and `algo/bw-support` explicitly excludes input-format work. Blocks the one-channel half of `nf-reconstruction/mono-input`

## Goal

Accept a SilverFast scan whose **primary image is 16-bit grayscale** rather than
3-channel RGB, carrying its IR page as before. Today nc refuses the file outright,
so B&W scans cannot enter the pipeline at all.

## Why this is its own task

`io/silverfast-decode` required `Gray(16)` only for the **IR plane** beside an RGB
`IFD0`; a gray *primary* was never in scope. And `algo/bw-support` — the mono
colour model — explicitly rules this out: *"16-bit RAW scan input is a separate
concern … do not pull input-format work into this task."* So neither existing task
owns it. (`bw-support` is now superseded; the consumer is
`nf-reconstruction/mono-input`.)

Measured on real assets (`../nc-assets/rolls/ILFORT-HP5-2026-08-10/`, seven Ilford
HP5 frames, 2026-08-11):

```
unsupported: … expected 3-channel 16-bit RGB in the primary image,
             found Gray(16); only SilverFast HDR/HDRi 16-bit scans are supported
```

Their structure is otherwise familiar — `IFD0` gray 16-bit 5184x3600, `IFD1`
reduced-resolution, `IFD2` full-resolution **"Transparency mask"**
(`NewSubfileType=4`, the marker the IR path already requires). So the IR plane is
present and marker-verified; only the primary's channel count differs.

## Open questions

1. ~~What does a gray primary become internally?~~ **Decided**
   ([`docs/design/mono.md`](../../design/mono.md)): one signal, carried as three equal
   channels by the 3×3. Whether this reader widens it or yields a one-channel `Scan` is
   `nf-reconstruction/mono-input`'s open question.
2. **The memory model.** `pipeline::memory` needs its own figures for a one-channel
   u16 scan widened to three f32 channels, measured rather than inherited.
3. **What identifies the file as a supported gray scan** rather than an arbitrary
   grayscale TIFF? The existing decoder leans on SilverFast XMP and the IFD
   layout; state which of those is load-bearing here.
4. **Reject or accept a gray primary with no IR page?** Both exist in the wild.

## How to Verify

- The HP5 frames decode, and `hanten inspect` reports their dimensions, IR presence
  and scanner metadata — derived numbers only, never sample pixels in context.
- The IR page is recognised as marker-verified (`NewSubfileType=4`), the same as
  on an RGB HDRi scan.
- Existing RGB HDR/HDRi fixtures are byte-identical through decode — this task
  adds an input shape, it must not perturb the shipped one.
- A gray TIFF that is *not* a SilverFast scan is refused with a clear message.

## Dependencies

- [SilverFast HDR/HDRi decode](silverfast-decode.md)

Blocks the one-channel half of
[Mono input in the decode](../nf-reconstruction/mono-input.md) (the three-channel
silver half does not need this reader), and gates the B&W half of
[IR usability by measurement](../film-base/ir-usability-detection.md).
