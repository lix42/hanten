# The print-contrast knob

## Goal

Print contrast becomes a look knob: the half of `gamma` the decode no longer
carries, pivoted at mid-grey and applied before the SDR/HDR branch.

## Design

- **It is one half of a split, not a new idea.** Part 1 splits `gamma` into
  linearizing the film (calibration, stays in the decode) and print contrast (a
  look, moves here); today's single value bundles both. So this knob starts life
  with a counterpart that must already have moved — hence the dependency.
- **Pivoted at mid-grey**, for the same reason the decode's anchor pins mid:
  contrast and exposure stay independent, and changing contrast pivots the image
  instead of moving it.
- **Scene-referred, before the branch.** Contrast is character below diffuse
  white, which the two renditions must agree on for a gain map to work.
- **It, not the operator, decides shadow contrast.** Measured on the shipped
  reinhard at its default headroom (Part 2, "The shadow end"): local slope 1.00 at
  0.002, 0.99 at 0.01, 0.82 at mid — below mid the operator is a gain, not a
  curve. Everything a viewer reads as shadow contrast therefore arrives from this
  stage or from the decode's linearization.
- **Its own key under `look`.** `nf-look/stage` settled the container on
  2026-09-23: one key per control, not one CDL-style object shared with the grade.

## Open questions

- The default. Today's 2.0 minus the ≈1.8 linearization is roughly a 1.10× print
  contrast, which is a starting point rather than a decision — values belong to
  `nf-calibration`.
- Whether contrast and the per-channel grade compose in a stated order, or the
  grade is defined as acting on the contrast's output.
- **Its overlap with the grade.** A pivoted grade with equal exponents is this knob,
  so the two can spell one operation twice. Decide who owns neutral contrast in
  whichever of the two tasks runs first.

## How to Verify

- Unity is a bit-exact identity.
- A mid-grey pixel is unmoved at any setting; a ramp steepens symmetrically about
  it.
- At matched lightness, a contrast change moves shadow separation where a fit
  range change does not — the measurement that motivates the knob's existence.
- The first look control has landed (`path-to-white`, highlight desaturation on by
  default): `LookSection` carries two predicates, `is_empty` (moves no pixel — what
  `applied()` reads) and `asks_for_a_look` (neither default nor empty — what a no-look
  destination such as `film-master` reads to refuse: the default is spared because
  every default recipe carries it, an empty look because it is an identity). This
  control extends both.

## Dependencies

- [The look stage](stage.md)
- [Splitting `gamma` into calibration and look](../nf-reconstruction/gamma-split.md)
