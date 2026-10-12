# More midtone contrast, the ends held by a toe and the shoulder

## Goal

If [`midtone-contrast-probe`](midtone-contrast-probe.md) finds that more midtone contrast
wins, build the toe that brings the dark end back: spread a frame's span over more look
stops than the display's range, then hold the ends with a toe and reinhard's shoulder, so
the white and the dark end stay about where they land today. If the probe loses, close
this task with no code.

## Design

Known:

- Earlier toes pushed black **down** to place it (`parametric-operator`; the old
  sigmoid's toe in `algo/reconstruction-render-curve-split`, dropped for costing black
  depth). This toe holds shadow detail **up** after a steeper midtone; that is untested.
- Two cases `nf-calibration/hybrid-slope-bounds` hands here, where content passes the
  span's targets with no slope at fault: the tails beyond a frame's own ends (its darkest
  and brightest 1 %), and a span wider than ~7.3 stops, where the floor slope (1.237) stops
  compressing.
- The overshoot's size and split between the ends come from the probe.

Open:

- The toe's shape, and where it sits: fit range is the natural stage, and it must keep
  below diffuse white identical across peaks.
- How it coexists with display black.
- Noise on thin frames: a steeper midtone raises grain in stops, and the maximum slope
  binds more often — `nf-calibration/short-span-protection` re-checks it after this task.

## How to Verify

- A review of the toe against the straight slope at matched midtone contrast, with shadow
  noise and the share of pixels near black measured, not only judged.

## Dependencies

- [Does more midtone contrast win?](midtone-contrast-probe.md) — whether this task is
  needed, and the overshoot it must bring back
