# The print-contrast knob

## Goal

Print contrast becomes a look knob: the half of `gamma` the decode no longer
carries, pivoted at mid-grey and applied before the SDR/HDR branch.

## What already exists

**The knob landed with its counterpart** (`nf-reconstruction/gamma-split`, 2026-09-24),
because the split could not ship half-done: `look.contrast` / `--contrast`, a power of
each ACEScg channel pivoted at mid-grey, default `2.0 / 1.8` (so the default render's
neutrals are unchanged), `1` the identity, running before highlight desaturation. Its
value rule, merge arm, report field and both `LookSection` predicates are in place. So
this task is what remains around the knob, not the knob.

Design points that still hold and that the remaining work must keep:

- **It, not the operator, decides shadow contrast.** Measured on the shipped
  reinhard at its default headroom (Part 2, "The shadow end"): local slope 1.00 at
  0.002, 0.99 at 0.01, 0.82 at mid — below mid the operator is a gain, not a
  curve. Everything a viewer reads as shadow contrast therefore arrives from this
  stage or from the decode's linearization.
- **Exposure is in scene stops and contrast expands it**: scene correction runs first,
  so `--exposure e` moves the graded image `e · contrast` stops.
- **Per-roll contrast lands here.** `docs/spike/white-placement.md`'s candidate C
  solves a per-roll `gamma`; under the split that is `look.contrast = gamma / 1.8`,
  never a per-roll linearization.

## Open questions

- **The default.** `2.0 / 1.8` reproduces the pre-split render; it is a starting point
  rather than a decision — values belong to `nf-calibration`, and a per-roll value is
  `nf-calibration/anchor-comparison`'s candidate C.
- **Its overlap with the grade.** A pivoted grade with equal exponents is this knob, so
  the two can spell one operation twice. Contrast landed first, so it owns neutral
  contrast; `nf-look/per-channel-grade` should be defined relative to it (e.g. exponents
  normalised to the neutral one) and state its order against it.

## How to Verify

- At matched lightness, a contrast change moves shadow separation where a fit
  range change does not — the measurement that motivates the knob's existence.
- A default change carries its evidence (a review round) and restates the
  neutral-equivalence test in `pipeline::look` against the new value.

## Dependencies

- [The look stage](stage.md)
- [Splitting `gamma` into calibration and look](../nf-reconstruction/gamma-split.md)
