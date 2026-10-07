# Settle the midtone line's fit range against the chart

## Goal

Choose the midtone neutral's fit range and fade width from a known-neutral reference,
and check that the neutrals the spike treated as ground truth are neutral.

## Design

Known (`docs/spike/poor-development.md`):

- Fitting the line on every voted band or only on bands below the fade made 09-20
  visibly warmer, neither better nor worse by eye; 09-29 barely moved. Patch medians
  disagreed with the eye twice at this scale, so they cannot decide it.
- The fade (1 stop below the roll's white) was never varied.
- The spike's neutral patches are hand-marked surfaces: cloud shade (grey or slightly
  blue?) and 09-18's information board (1802, white?) are assumptions.
- The chart rolls carry ordinary picture frames (`analysis/calibration-frame-capture`),
  so a roll's line is fitted from its pictures and checked on its chart's neutral row
  across the bracket — five or more densities at one light.

Open:

- Whether a well-developed chart roll can decide the fit range at all, when the gap
  between the variants is largest on poorly developed rolls.
- How the chart frames are kept out of the line's votes
  (`analysis/calibration-role-consumers`).

## How to Verify

- The chart's neutral row, rendered with each variant, gives a residual per stock; the
  chosen fit range and fade are the ones with the smaller residual, or the spike's values
  are kept with that reason recorded.
- The spike's marked neutrals re-scored against the chart's.

## Dependencies

- [A midtone neutral measured per roll](midtone-neutral.md) — the line this tunes
- [Capture the calibration frames](../analysis/calibration-frame-capture.md) — the
  known-neutral reference
