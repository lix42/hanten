# Re-fit the highlight-desaturation band under the chosen white and black

## Goal

Re-place highlight desaturation's band and strength under the white rule
`nf-calibration/anchor-comparison` chose, which decides which pixels reach the operator's
range, and judge the result with a black point in the chain. The shipped values were
fitted provisionally, under a hand-set per-roll contrast.

## Design

What is known:

- **The values were provisional by design.** [`path-to-white`](path-to-white.md) placed the
  band `0.015 → 0.025` (strength 0.8) under candidate C's hand-set contrast, and said its
  parameters are re-fitted once the anchor rule is chosen. It has been:
  `docs/progress/nf-calibration.md`, 2026-09-25.
- **The chosen rule is lower in contrast than C on most rolls** (whole contrast 2.23–2.97,
  against C's 2.32–6.61), so a different population of pixels lands in the band. A contrast
  move steepens everything below white, which is the same reason `path-to-white` refused to
  carry over the spike's values.
- **Whites carry more chroma as contrast rises, and the operator does not take it back.**
  Contrast is a per-channel power, so it multiplies residual cast along with saturation. On
  09-18, marked whites' mean C\* was 3.8 at gamma 2.0, 5.5 under the rule and 7.8 at 4.15,
  with colour patches rising in step.
- **The black point does not change what reaches the band.** It lands in fit range, after
  the look. It is a dependency because the re-fit is judged by eye, and
  `anchor-comparison` found every render judged without black misleading: all of them
  looked pale, and the review would have tuned around that.

Open:

- Whether the band's position, its strength, or both move.
- Whether chroma that grows with contrast is the band's to remove at all, or the per-channel
  grade's ([`per-channel-grade`](per-channel-grade.md)), since the operator reaches only
  highlights.

## How to Verify

`path-to-white`'s pair check, re-run under the rule and the black point: marked whites'
C\* falls as strength rises while a marked saturated patch does not, with luminance held.
The review set is judged with the black point in place.

## Dependencies

- [`measure-roll` places the roll's white](../nf-calibration/roll-white-rule.md) — the white
  the band is fitted under
- [A parametric operator with a toe](../nf-display-stages/parametric-operator.md) — the
  black point the re-fit is judged with (not an input to the band)
