# Protect short-span frames from too much contrast

## Goal

A guard that keeps a frame with a short span (thin, flat, hazy or night) from being
stretched so far that its grain dominates or its densest sample overflows. Today that is
the fixed `MAX_SLOPE` (3.5), which every placement shares; the question is whether a guard
keyed on the frame (its span, its noise) does better than one number.

## Design

Known (`nf-calibration/hybrid-slope-bounds`, its progress entries; numbers in
`../temp/hybrid-slope-bounds/sweep.log`):

- A placement's slope is about a fixed number over the frame's span (9.049 / span under
  `frame`, 7 / span under the hybrid), so a fixed maximum already binds only short spans.
  It bounds the base slope; `look.contrast` still multiplies past it.
- Noise follows the slope, about +0.2–0.4× the scan's own floor per +0.5, and thin frames
  sit higher at the same slope (at 2.5: 1.5–2.6× against 0.6–1.6×).
- Under the hybrid the thin frames never reach 3.0 on the archive; the maximum binds 1612,
  1719, 1799 and 1813. Under `frame` it binds often, and the thin frames at 3.5 reach
  2.5–3.9× the floor (1696's shadows 4.45×).
- Uncapped, 1612 and 1719 (spans 1.5–1.7 stops, slopes 5–6) overflow the densest sample
  and are refused: the guard is also what keeps them renderable.
- A guard has no switch (design-spec §6), so this is a rule, not a knob.

Open:

- Fixed or keyed on the frame — its span, its measured noise, its thinness — and a hard
  cap or a soft one.
- What a toe (`nf-display-stages/midtone-contrast-toe`) leaves to do: on thin frames 76–91 %
  of pixels render under L\* 30, the toe's region.

Outcome (2026-10-13): fixed, hard, 3.5, on the placement's slope; `look.contrast` passes
it. Nothing stored predicts grain, so no frame-keyed guard. Details in the progress log.

## How to Verify

- A review on the thin and capped frames of the archive under `hybrid` and `frame`, judged
  on grain, with the noise measure beside each arm.

## Dependencies

- [The maximum slope](hybrid-slope-bounds.md) — the measurements and the fixed guard this
  task replaces or keeps
- [More midtone contrast, the ends held by a toe and the
  shoulder](../nf-display-stages/midtone-contrast-toe.md) — a toe changes the shadows'
  contrast, where thin frames' grain shows
