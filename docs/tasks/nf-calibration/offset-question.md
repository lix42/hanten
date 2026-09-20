# Does `density.offset` earn a value?

## Goal

Decide whether the decode's per-channel density offset earns a non-zero value or
stays at zero. A recorded "zero, and here is why" is a complete outcome.

## Design

What is known:

- **The term is physically real.** It is the residual between the base measured
  from the rebate and the density at which the three layers correspond to equal
  exposure — the layers' toes start at different exposures — and the datasheets
  carry exactly that term, per stock.
- **Two candidate values were rejected on 2026-09-17** (design-update Appendix E):
  a datasheet-fitted pair and one re-fitted to our own marked patches. The verdict
  rejected the *values*, not the term.
- **Why the patches cannot identify it.** They are white surfaces, only
  approximately neutral, and their density range is confounded with illumination —
  so they anchor a level and not a slope, and the offset is a level. With the base
  left free, base and offset are degenerate; with it measured, the offset is
  identifiable in principle, which is why it was worth a render not an argument.
- **`scale` is settled first.** Whatever `scale` cannot remove is the evidence for
  what an offset would have to do, so read the residual the loop leaves rather than
  fitting both at once.

Open:

- **What measurement identifies a value?** Density varied at a single illuminant —
  one surface at two or more densities *in the same frame*, never pooled across
  frames, since every reference that re-balances per frame turns a per-frame gain
  into a per-frame density offset.
- **Whether the answer is per stock or global.** The datasheet fits split by tier
  (blue spanning roughly −0.002 to −0.101), which is what made the gain look
  stock-independent and the offset not.
- Whether a rejected term should stay a knob at all, or be removed from the
  surface until a value exists.

## How to Verify

- A written verdict — value, or zero with its reasoning — resting on a measurement
  that varies density at one illuminant, not on patch arithmetic alone.
- If a value ships, the same whole-set visual review that rejected the last two,
  under one fixed rendering, with the result recorded either way.

## Dependencies

- [Tune `scale` and `gamma` by review](scale-gamma-loop.md) — the offset is read
  off what `scale` leaves behind
