# More midtone contrast, the ends held by a toe and the shoulder

## Goal

Find out whether an S-curve renders better than today's straight slope: spread a
frame's span over more look stops than the display's range (the slope's target plus
about 2 EV), then bring the ends back with a toe and reinhard's shoulder. Midtones gain
contrast; the white and the dark end stay about where they land today.

## Design

Known:

- Today the placements spread a span over `SPAN_LOOK_STOPS` (9.049) or the hybrid's
  R, and nothing below mid-grey compresses: reinhard is nearly a gain there and display
  black (off by default) only pushes the base down. A steep slope's darkest tail sinks
  past −6 look stops (L\* ≈ 3) into visual black — the shadow detail 1719 loses.
- Earlier toes pushed black **down** to place it (`parametric-operator`; the old
  sigmoid's toe in `algo/reconstruction-render-curve-split`, dropped for costing black
  depth). This toe holds shadow detail **up** after a steeper midtone; that is untested.
- Two cases `nf-calibration/hybrid-slope-bounds` hands here, where content passes the
  span's targets with no slope at fault: the tails beyond a frame's own ends (its darkest
  and brightest 1 %), and a span wider than ~7.3 stops, where the floor slope (1.237) stops
  compressing.
- The reviews have leaned towards restraint: `span-roll-slope` set no white target and a
  brighter white with mid-grey pinned lost in `parametric-shoulder`.

Open:

- Does more midtone contrast win at all? Probe first with today's binary (a contrast
  multiplier, if it applies under the frame placements), ends left to clip, before any
  toe exists.
- The overshoot's size and its split between the ends: the white already has reinhard's
  headroom, so most of it may belong to the dark end. Estimate from data.
- The toe's shape, where it sits (fit range is the natural stage; it must keep below
  diffuse white identical across peaks), and how it coexists with display black.
- The hybrid's own span target, R (`roll.hybrid_span_stops`, 7; `--hybrid-span`, no code
  to review it): the same lever for the default placement. Reviewed twice without a toe
  (the spike's a4, `envelope-hybrid-placement`); a toe may let it rise. The hybrid's
  darkest 1 % sits at L\* 10.6–14.3 (per-roll medians), `frame`'s at 8.2, CCR's at 4.7–7.4
  (`nf-calibration/hybrid-slope-bounds`) — less contrast than a per-frame slope by design.
- Noise on thin frames: a steeper midtone raises grain in stops, and the maximum slope
  binds more often — `nf-calibration/short-span-protection` re-checks it after this task.
- Colour: the placements hold colour at the roll's slope, so saturation does not follow
  the steeper midtone unless decided.

## How to Verify

- The contrast probe reviewed on 09-18 and 09-29 under the default placement; if it wins,
  a toe's review against the straight slope at matched midtone contrast, with shadow
  noise and the share of pixels near black measured, not only judged.

## Dependencies

- [The maximum slope](../nf-calibration/hybrid-slope-bounds.md) — the slope this task
  steepens is bounded there; its values are the baseline
