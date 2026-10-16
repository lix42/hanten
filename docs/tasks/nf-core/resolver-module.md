---
title: Give parameter resolution its own module
status: done
deps: []
related: [nf-core/three-step-pipeline]
summary: move Recipe::shared_params and the resolved_* methods out of recipe.rs
done: 2026-10-15
---
# Give parameter resolution its own module

## Goal

Give the step that combines the roll's measurements and the user's style into stage
params one home in code. `nf-core/three-step-pipeline` kept the stages defined by
operation and made that combination where the two meet (design-spec §6, "Two
renderings"); the decode is not part of it — it supplies the image and the decoded film
base (`chain::render`'s arguments). Today the combination is
`Recipe::shared_params` and the `resolved_*` / `applied_*` methods spread through
`src/recipe.rs`, which also parses, merges and validates.

## Design

Known:

- Pure functions from the recipe (and the rendering's `Base`) to the stages' params, in
  their own module; `Recipe` keeps parsing, merging and validation.
- A refactor: no pixel, report or recipe change. The goldens and the drift gate must not
  move.

Decided (user, 2026-10-15):

- The module is `recipe::resolve`, free functions of `&Recipe`: a submodule, since
  validation reads resolved values.
- The report's provenance (`roll_report`, `RollReport`, `LookReport`, `ResolvedSlope`,
  `SlopeBase`) moves with it.
- `for_frame` moves to `recipe::compose` as the frame's first layer: it produces a recipe,
  before the flags and validation, not stage params.

## How to Verify

- Every CI gate green, with no golden, fingerprint or snapshot change.
- `src/recipe.rs` no longer holds the resolution methods.
