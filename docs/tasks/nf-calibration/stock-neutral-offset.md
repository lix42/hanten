---
title: A per-stock correction from the charts
status: todo
deps: [analysis/calibration-frame-capture]
related: [nf-calibration/offset-question, io/scanner-density-calibration]
summary: remove the part of the residual cast fixed per stock, measured from chart rolls, so the roll white balance is left with the light
---
# A per-stock correction from the charts

## Goal

Remove the part of the residual cast that is the stock's own, the same on every roll of
it, so that what the roll's (or a frame's) white balance still removes is the light. A
declared stock would then render greys at a\* = b\* = 0 (CIELAB) with the roll colour correction off
(`--colour-correction off`), and keeping the illuminant cast becomes a real choice.

## Known

- `2026-10-12-Gold200` (`docs/reports/chart-cast.md`): after the bundled density scale,
  blue sits about 0.044 density below green across the straight part, with the fitted
  density scale against green 2–5 % below 1. A blue density offset of that size, with the
  roll colour correction off, renders the greys at C\* 2–3 across the whole bracket
  (in-sample).
- With the roll colour correction on, an offset changes little: `measure-roll` absorbs it
  into the roll white balance (blue 1.254 → 1.072), and the greys came out within 0.4 b\*
  of the run without it. The value only matters once the roll colour correction is off
  or opt-in.
- One frame cannot separate a per-stock residual cast from the light: both shift log
  exposure per channel. Only outside knowledge can — a chart under known light, or the
  same stock repeated under different lights and developers.
- The film base already removes the scanner lamp's colour (a constant transmission
  ratio), so the leftover is the stock, the processing and the light.

## Open

- **Density or log-exposure side?** A density offset (`reconstruction.offset`) stays
  constant down to the film base; a shift in log exposure (a layer's speed, like the
  light) fades in the toe. Gold 200's blue residual shrinks slightly toward the film base. This is
  `offset-question`'s verdict to give, and it decides where the value lives.
- **Removed by default, or an opt-in?** The user (2026-10-15) is unsure whether the
  per-stock residual cast is stock character worth keeping. Against keeping it: printing
  removed a constant per-channel shift with the filter pack, and on Gold 200 it renders
  the greys +12 b\* yellow under bluish light. Decide before shipping.
- **How a stock is declared**: a flag and recipe key, and stock data the code reads at
  run time (today's `film_stock/` is test-only evidence).
- **How much is the developer**: this roll was an early roll of a CineStill Cs41 kit.
- A stock with no chart calibration has no value, so the roll colour correction must stay
  on for it.

## How to Verify

- Two or more chart rolls of one stock, in different light, agree on the value within a
  stated tolerance (`docs/reports/chart-cast.md`).
- With the value declared and the roll colour correction off, a held-out chart roll's greys read
  within that tolerance of a\* = b\* = 0, apart from its light.
- The pictures of those rolls reviewed by eye under the change (`render-review-set`).

## Dependencies

- [Capture the calibration frames](../analysis/calibration-frame-capture.md) — more chart
  rolls, per stock and light
