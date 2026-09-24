# Retire the `characteristic` curve path

## Goal

Remove the per-stock characteristic curve from reconstruction, so that the claim
the other retirements make — the fixed decode is the only reconstruction — is
actually true.

## Design

- **What survives unowned today.** [Retire the sigmoid and `simple`](sigmoid-and-simple.md)
  removes two of the three `DensityCurve` members; the third, with
  `CharacteristicParams`, the curve-inversion path in `src/algo/film_stock`,
  `--film-stock`, three `ConversionPreset` names (`characteristic-generic`,
  `-stock`, `-aim`) and `DensityParams::default_scale_for`'s `[1, 1, 1]` arm, is
  nobody's. This task is that third member.
- **Collapsing the curve set collapses `default_scale_for`.** `DensityParams::default_scale_for` records its
  per-curve default being resolved in **three** places — the recipe's `Deserialize`
  (off raw-JSON key *presence*), the `--density-curve` merge arm (which must stay
  before the `--density-scale` arm), and the `roll` planner by hand, because the
  overlay is merged onto the *serialized* shared config. With one curve there is no
  per-curve answer left: decide whether `density.scale` keeps a plain default and
  delete the machinery, rather than leaving a one-armed match and a roll warning
  about a reset that can no longer happen.
- **The stock *data* is not this task's to delete.**
  [A home for the film-stock data](../nf-look/stock-data-home.md) owns the curve
  tables, the registry and `docs/datasheets/` — and Part 1 quotes those tables as
  the evidence for the decode's fixed constants. This task removes the inversion
  *path*; the two must agree on `--film-stock`'s fate before either lands.
- **Retiring a curve is mostly deleting conditions.** `DensityCurve::consumes_reference`,
  `cli::unconsumed_dmax_warning` and the `--film-stock` required-here/refused-there pair
  all exist to tell this curve from the parametric ones; each deletion is a rule that can
  no longer be ordered wrongly. (`takes_dmax()` and `cli::preset_curve`'s
  carry-`dmax`-across rule were the same shape and went in
  `core/calibration-recipe-section`, which moved the reference out of the curve.)
- Removed preset names and the removed curve value get migration errors on the
  `--algorithm` precedent — no aliases. Note `characteristic-generic` was
  `algo/split-default-migration`'s target, so any doc calling it the next default is
  stale.

## Open questions

- Does `--film-stock` keep a meaning as recorded provenance, or go and return with
  per-stock normalization? (Shared with `stock-data-home`; one answer, not two.)
- Does the decode keep a `density.scale` default at all once it is not per-curve?

## How to Verify

- `reconstruction.curve` accepts only the fixed decode; a recipe naming
  `characteristic`, and each of the three preset names, fails with a message naming
  the replacement.
- Nothing resolves a per-curve `density.scale`: all three sites are gone or reduced
  to one unconditional default, proven through the real path (`merge`, the binary, a
  roll with a per-frame override), not by calling the resolver.
- No error message or help text recommends a flag or preset that no longer exists.
- The four CI gates pass.

## Dependencies

- [Retire the sigmoid and `simple`](sigmoid-and-simple.md)
- [A home for the film-stock data](../nf-look/stock-data-home.md)
