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
this task settled what surrounds the knob, not the knob.

Design points that hold:

- **It, not the operator, decides shadow contrast.** Measured on the shipped
  reinhard at its default headroom (Part 2, "The shadow end"): local slope 1.00 at
  0.002, 0.99 at 0.01, 0.82 at mid — below mid the operator is nearly a gain, not
  a curve. Everything a viewer reads as shadow contrast therefore arrives from this
  stage or from the decode's linearization.
- **Exposure is in scene stops and contrast expands it**: scene correction runs first,
  so `--exposure e` moves the graded image `e · contrast` stops.
- **Per-roll contrast lands here.** `docs/spike/white-placement.md`'s candidate C
  solves a per-roll `gamma`; under the split that is `look.contrast = gamma / 1.8`,
  never a per-roll linearization.

## Decisions (user, 2026-09-24)

- **One knob, holding whatever makes the roll right.** No `k_roll × k_taste` split: the
  chain already asks a reader to carry `linearization × contrast`, and a third factor
  makes it harder to follow. Under candidate A that is the fixed default; under C, or D
  where its cap does not bind, it is the roll's solved value, carried in the recipe as
  the roll's white balance is. Taste is editing that number, per roll.
- **The default stays `2.0 / 1.8`, provisional.** It reproduces the pre-split render and
  places no roll's white; `nf-calibration/anchor-comparison` decides whether a fixed
  value survives at all. This task does not change it.
- **Look presets do not set contrast** — under C/D a preset would overwrite the roll's
  solved value (recorded in `look-presets`).
- **Exposure is not a contrast control.** It is a level move; its effect on the shadow
  slope comes only from reinhard's mild curvature below mid — a few percent, growing as
  exposure lifts shadows toward mid. Contrast sets the slope 1:1.
- **Out of scope here; `anchor-comparison` owns it:** which percentile defines the
  roll's white `W`, and whether it is a code constant or a recipe value.
- **The grade overlap.** Contrast owns neutral contrast; `per-channel-grade` must not
  be able to move it, and picks its own form for that. The grade runs after contrast and
  before highlight desaturation, and its cast grows with contrast — the reasoning is in
  that task file.

## How to Verify

- At matched lightness, a contrast change moves shadow separation where a fit
  range change does not — the measurement that motivates the knob's existence.
- If a later task moves the default, the change carries its evidence (a review round)
  and restates the neutral-equivalence test in `pipeline::look` against the new value.

## Outcome (2026-09-24)

- **Shadow separation is the contrast's.** A neutral ramp through the whole chain
  (`chain::tests::contrast_not_fit_range_decides_shadow_separation`), mid-grey at 0.18
  in every cell: the log-log slope from 0.005 to 0.05 is the contrast exactly with fit
  range off, and 0.98–0.99× the contrast with it on; headroom 2, 3 and 6 stops move it
  by under 0.003. Contrast 1 / 1.11 / 1.5 gives 0.98 / 1.09 / 1.49 at the default
  headroom. Synthetic only: both operators are per-pixel, so a real frame's neutrals
  would show the same function.
- The decisions above; no default pixel moved.

## Dependencies

- [The look stage](stage.md)
- [Splitting `gamma` into calibration and look](../nf-reconstruction/gamma-split.md)
