---
title: Revisit the colour defaults against the chart record
status: todo
deps: [analysis/calibration-frame-capture]
related: [nf-calibration/stock-neutral-offset, nf-scene-correction/midtone-neutral-fit, nf-calibration/neutrality-gate]
summary: "once more chart rolls are in docs/reports/chart-cast.md: white-balance form, midtone line, and whether a calibrated stock still needs the roll colour correction on"
---
# Revisit the colour defaults against the chart record

## Goal

Once more chart rolls are in `docs/reports/chart-cast.md`, decide whether the colour
defaults should move — and move them only with that evidence, recorded either way.

## Known (one roll, `2026-10-12-Gold200`)

- The default (gain white balance, midtone line on) leaves the chart's greys at b\* −15.
- `tilt` with the midtone line off is the best setting measurable without a chart
  (C\* 3.4–4.0), but it wins by applying less of an oversized white-balance gain in the
  shadows, not by fixing a density-scale error; on a roll whose gains fit it would
  under-correct the shadows.
- The midtone line's red and blue terms cancel on a\*, and each alone overshoots.
- Placement and highlight desaturation leave the residual cast alone.

## Open

- Does `tilt` with the line off win on every chart roll, and for the same reason?
- Does the midtone line help on any roll, or only on the pictures it was fitted on
  (`nf-scene-correction/midtone-neutral-fit`)?
- With `stock-neutral-offset` in place, should a calibrated stock render with the roll
  colour correction off (`--neutral-balance off`) by default, keeping the illuminant cast?
- The chart frames judge greys, not pictures: any change also needs a review of the
  rolls' pictures by eye.

## How to Verify

- A recorded decision per question, resting on at least two chart rolls, with the
  pictures reviewed (`render-review-set`); a moved default adds a
  `PIPELINE_FINGERPRINTS` row and updates `docs/using-nc.md`.

## Dependencies

- [Capture the calibration frames](../analysis/calibration-frame-capture.md) — the chart
  rolls this decision needs
