---
title: Confirm the thin lift on an independent roll
status: closed
deps: [nf-calibration/taste-vs-quality]
tags: [new-flow]
closed_into: nf-calibration/envelope-hybrid-placement
---
# Confirm the thin lift on an independent roll

> **From TASKS.md** (migrated 2026-10-14): **closed 2026-10-11, superseded**: the hybrid placement replaced both lifts by default, so the thin lift renders only under `--placement roll`; the hybrid gives 09-29's two thin frames a steeper slope than the lift, and `hybrid-slope-bounds`' sweep measured its noise
>
> **Dependency note** (migrated 2026-10-14): **closed—superseded** 2026-10-11 by `nf-calibration/envelope-hybrid-placement`; the dep below is decision history filed 2026-10-02: thin-frame-lift's confirmation round and noise measurement, compared through the thin lift's own switch

> **Closed, superseded, on 2026-10-11.** Since `envelope-hybrid-placement` the default
> placement replaces both lifts; the thin lift renders only under `--placement roll`. The
> hybrid gives 09-29's thin frames a steeper slope than the lift would, and
> `hybrid-slope-bounds`' noise sweep measured it; grain on thin frames is
> `short-span-protection`'s. Numbers in the progress log. This file is kept so existing
> references resolve.

## Goal

Confirm `thin-frame-lift`'s thresholds and slope bound on a roll that played no part in
choosing them, and measure the noise the steeper slope adds. Carried over from
`thin-frame-lift` (closed 2026-10-01 without either) through `taste-vs-quality`, which
made the lift a preference with its own switch (`--thin-lift off`).

## Design

Known:

- **2026-09-29 Ektar 100** (31 frames, in `nc-assets`) was scanned after the thresholds
  were set. `measure-roll` thin-lifts four of its frames: 2017, 2044, 2052, 2053 (slopes
  1.78–1.90, none bounded).
- The lift is taste: a round compares `--thin-lift off` (the frame's small lift) with on,
  and "brighter, no preference" is an expected verdict, not a failure.
- **09-29 is a poorly developed roll** (`docs/spike/poor-development.md`), and its frames
  sit near the threshold: under the spike's two-point scale 2044 lost its thin lift
  (16 L\* darker). A colour correction that moves the whites (`midtone-neutral`'s two-pass
  white) or the placement (`span-roll-slope`'s steeper roll slope, or
  `envelope-hybrid-placement`, which generalises the lifts) can
  change which frames qualify, so a round states the build it ran on.
- A low-contrast frame with no shadows near the base (09-18's 1799, span 1.6) is not thin
  by the rule and gets only the small lift.
- No noise has been measured at slopes up to 2.11 (2.4 was judged by eye only, on 2005 and
  1983); `nf-look/scene-range-mapping`'s 09-25 noise budget preferred a flatter whole
  contrast for a lone dark frame.

Open:

- Whether the four frames are thin by underexposure or by scene, and whether any frame
  the rule misses looks like it should qualify.
- The noise measure and its budget (`nctool metrics` on output pixels).
- Whether the thresholds or the slope bound move.

## How to Verify

- A review round on 09-29's thin frames, on vs off, with verdicts recorded.
- Noise measured at each lifted frame's slope against its unlifted render.

## Dependencies

- [Separate taste from quality](taste-vs-quality.md) — the switch a round compares
  through
