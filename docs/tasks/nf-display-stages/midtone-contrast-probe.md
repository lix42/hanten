---
title: Does more midtone contrast win?
status: done
deps: [nf-calibration/hybrid-slope-bounds]
tags: [new-flow]
summary: not as the default (×1 preferred)
done: 2026-10-11
---
# Does more midtone contrast win?

> **From TASKS.md** (migrated 2026-10-14): **done 2026-10-11:** not as the default (×1 preferred); ×1.29 is an opt-in, documented as `--contrast 1.3`, its dark end sinking (darkest 1 % near L\* 6). Spike: [`docs/spike/midtone-contrast-probe.md`](spike/midtone-contrast-probe.md)
>
> **Dependency note** (migrated 2026-10-14): split from `midtone-contrast-toe` 2026-10-11: whether more midtone contrast wins, probed with the ends left to clip before any toe exists

**Done:** 2026-10-11 — the default stays at ×1; ×1.29 is an opt-in, `--contrast 1.3`
(`docs/spike/midtone-contrast-probe.md`).

## Goal

Find out, before any toe exists, whether spreading a frame's span over more look stops
than the display's range renders better than today's straight slope, and if it does, by
how much and at which end the overshoot belongs. The answer gates
[`midtone-contrast-toe`](midtone-contrast-toe.md) and everything after it.

## Design

Known:

- Today the placements spread a span over `SPAN_LOOK_STOPS` (9.049) or the hybrid's
  R (7), and nothing below mid-grey compresses: reinhard is nearly a gain there and display
  black (off by default) only pushes the base down. A steep slope's darkest tail sinks
  past −6 look stops (L\* ≈ 3) into visual black — the shadow detail 1719 loses.
- The idea under test: a target of the span plus about 2 EV, the ends brought back later
  by a toe and reinhard's shoulder. Here the ends are left to clip, so the probe judges
  the midtones only.
- The reviews have leaned towards restraint: `span-roll-slope` set no white target and a
  brighter white with mid-grey pinned lost in `parametric-shoulder`.

Open:

- Whether `look.contrast` multiplies under the default (hybrid) placement as the probe
  needs, or whether the placement's guards re-solve the slope — in which case the probe
  needs a throwaway spike hook.
- Whether more midtone contrast wins at all, and at what step.
- The overshoot's size and its split between the ends: the white already has reinhard's
  headroom, so most of it may belong to the dark end. Estimate from data.

## How to Verify

- A review on 09-18 and 09-29 under the default placement, today's slope against one or
  two steeper ones, ends left to clip; the clipped share at each end measured beside each
  arm.

## Dependencies

- [The maximum slope](../nf-calibration/hybrid-slope-bounds.md) — the slope this probe
  steepens is bounded there; its values are the baseline
