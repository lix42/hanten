---
title: More midtone contrast, the ends held by a toe and the shoulder
status: done
deps: [nf-display-stages/midtone-contrast-probe]
tags: [new-flow]
summary: the dark floor lifts a frame's darkest 1 % to no deeper than 4.5 stops below mid-grey
done: 2026-10-12
---
# More midtone contrast, the ends held by a toe and the shoulder

> **From TASKS.md** (migrated 2026-10-14): **done 2026-10-12** (`pipeline_version` 15): the dark floor, `default`'s, lifts a frame's measured darkest 1 % to no deeper than 4.5 stops below mid-grey (none at ×1, most at ×1.3); display black became the other setting of the one key, `fit_range.dark_end`. `--contrast 1.3` with it is acceptable under `roll` and `frame` (bright under `frame`), opt-in only
>
> **Dependency note** (migrated 2026-10-14): filed 2026-10-11: steepens the slope `hybrid-slope-bounds` bounds, so its maximum is the baseline and is re-checked after; narrowed to the toe 2026-10-11; the probe kept the default slope, so the toe holds the opt-in `--contrast 1.3` and the default's tails

## Goal

Hold the dark end up wherever it sinks into black, with a toe beside reinhard's shoulder:
under a steeper slope, first the opt-in `--contrast 1.3` the probe settled on, and in the
default's own deep tails. The default slope stays as it is.

## Design

Known:

- The probe (`docs/spike/midtone-contrast-probe.md`): the default stays at ×1; ×1.29 was
  liked as an opt-in, judged with its shadows sinking — its darkest 1 % renders near L\* 6
  instead of 12, and the dark end takes an estimated median 58 % of the extra stops. The white is
  already held by the shoulder.
- Earlier toes pushed black **down** to place it (`parametric-operator`; the old
  sigmoid's toe in `algo/reconstruction-render-curve-split`, dropped for costing black
  depth). This toe holds shadow detail **up** after a steeper midtone; that is untested.
- Two cases `nf-calibration/hybrid-slope-bounds` hands here, where content passes the
  span's targets with no slope at fault: the tails beyond a frame's own ends (its darkest
  and brightest 1 %), and a span wider than ~7.3 stops, where the floor slope (1.237) stops
  compressing. A steep frame's tail at ×1 also sinks (1719).

Settled (2026-10-12, review round 1 and the user):

- The toe is **the dark floor**, in fit range: a frame's measured darkest 1 % lifted to no
  deeper than 4.5 stops below mid-grey, on by default (`pipeline_version` 15). Shape S
  (display black's curve, reversed) over T: very hard to tell apart, S for a little more
  shadow detail. It acts below mid-grey only, so every peak still agrees below diffuse
  white. At ×1 it lifts none of the reviewed frames; the default slope stays (the probe).
- Display black and the floor are one key, `fit_range.dark_end`, refused together.

- `--contrast 1.3` with the floor is acceptable under `--placement roll` and `frame`
  (bright under `frame`, a user's choice), opt-in only. Under `frame` it is refused on
  11 short-span frames by the densest-sample probe, filed as
  [`nf-calibration/overflow-probe-scope`](../nf-calibration/overflow-probe-scope.md).

Open, for the dependents:

- Noise on thin frames: a steeper midtone raises grain in stops, and the maximum slope
  binds more often — `nf-calibration/short-span-protection` re-checks it after this task.

## How to Verify

- A review of the toe against the straight slope at matched midtone contrast (×1 and
  ×1.29), with shadow noise and the share of pixels near black measured, not only judged.
- Then `--contrast 1.3` under `--placement roll`, where low- and high-key frames are
  pushed further than under the hybrid; then `frame`, whose pinned white it moves.

## Dependencies

- [Does more midtone contrast win?](midtone-contrast-probe.md) — the opt-in contrast this
  toe holds, and how far its dark end sinks
