# A per-channel grade with a mid-grey pivot

## Goal

The look stage carries a per-channel adjustment pivoted at mid-grey — the
photographer-facing counterpart of the decode's `scale`, acting on working-space
channels after the 3×3.

## Design

- **It addresses the symptom, not the error, and that is the point.**
  Reconstruction's `scale` acts on film layers before the 3×3, so moving one
  channel there moves all three output channels: accurate, and unpredictable to
  tune by hand. A grade after the matrix moves the channel a photographer is
  looking at (design-update Part 2, "A per-channel grade with a pivot").
- **The pivot is what makes it more than white balance.** Without one, a
  per-channel power moves neutral everywhere; pivoted at mid-grey, a neutral mid
  stays neutral and the cast grows away from mid in both directions — which is
  exactly what white balance cannot do.
- **It subsumes `shadow_balance` / `highlight_balance`.** Per-channel adjustment
  by tone region is the same family, and one control beats three. Those two are
  also the non-monotone term in today's chain (Part 1), so retiring them into this
  is a monotonicity win as well as a simplification.
- **It does not replace the calibration.** Without a measured `scale` every roll
  needs hand-grading; this is a grade on top of a calibrated decode.
- **Needs a guard at and below zero.** A wide-gamut linear working space contains
  negative components, and a fractional power of a negative is NaN. Decide the
  behaviour deliberately — clamp, reflect through the pivot, or pass through — and
  report which; a NaN reaching the encoder is only counted, not explained.

## Open questions

- The grade's form inside its `look` key: a pivoted per-channel power, or a
  lift/gamma/gain triple per channel? The container is settled (`nf-look/stage`,
  2026-09-23): one key per control under `look`, not one CDL object — CDL's slope
  is white balance and its offset the flare subtraction, both scene correction's.
- **Its overlap with contrast.** Equal exponents on all three channels, pivoted at
  mid, *are* contrast — the same duplication that ruled out CDL. Decide who owns
  neutral contrast (e.g. the grade is constrained to keep a neutral neutral, or
  defined relative to one channel) in whichever of the two tasks runs first.
- Is the pivot fixed at 0.18 or stated? Fixed is the honest default while the
  decode pins mid.

## How to Verify

- Unit exponents are a bit-exact identity.
- A neutral mid-grey pixel stays neutral at any setting; departure from neutral
  grows away from mid in both directions on a synthetic ramp.
- Zero and negative components stay finite, in the documented way.
- A synthetic cast measurably shrinks, read back with `nctool metrics`.
- If this is the **first look control to land**: `LookParams` gains a "non-empty"
  predicate, and `applied()` and any destination that runs no look (`film-master`,
  once the new flow has one) read it — one rule, never one per knob
  (`nf-look/stage`). A look key on a no-look destination refuses, naming the look
  — if the new flow has one by then; otherwise `nf-destinations/preset-set`
  verifies it.

## Dependencies

- [The look stage](stage.md)
