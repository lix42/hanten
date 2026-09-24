# Retire the `shoulder` and `none` tones

## Goal

Leave fit range as the one display operator: remove `shoulder` and `none` from
`print.display_tone`, and `highlight_compress` with them.

## Design

- **Both exist for reconstructions already bounded at white.** `shoulder`
  flattens everything above 1.0, and `none` is the bypass that only works because
  the sigmoid pinned diffuse white at reference white. Once the decode is
  unbounded and fit range carries the character, neither describes anything.
- **`highlight_compress` currently means two different things** — a soft clip in
  the legacy print path, and the knee placement `0.5 + 0.25/(1 + hc)` on the
  display branches. So it cannot be deleted until the legacy retirement has
  landed: removing the display half first would leave the legacy meaning as the
  only one, and the knob would quietly change meaning instead of disappearing.
  Its `KneeWidth` / `Headroom` newtypes leave with it. (That retirement landed
  2026-09-23: the knob now has only its display meaning.)
- **Keep the over-range refusal, or state why not.** `none` is self-policing:
  gamut mapping, the transfer encode and each renderer's per-pixel range check
  still run under it, and the two ceilings differ (`1.0` for SDR,
  `LINEAR_HEADROOM` for HDR). Whatever remains must keep that behaviour or
  document the replacement — an unbounded value reaching the encode silently is
  the failure mode this guarded.
- **Don't preserve the enum for its own sake.** If one operator remains,
  `DisplayToneCurve` may stop being a selector; if the parametric operator
  arrives it returns with different members.

## How to Verify

- The flag and the recipe key reject the removed names with a removed-value
  error, not a parse failure listing them.
- No `validate` rule, report field or error message still names a tone that does
  not exist — grep for the negation of the claims, not for the changed code.
- An over-range render is still refused on the SDR branch and still renders on
  HDR.

## Dependencies

- [Retire `legacy` and `custom`](legacy-custom.md)
- [Fit range as a stage](../nf-display-stages/fit-range.md)
