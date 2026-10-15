---
title: Give parameter resolution its own module
status: todo
deps: []
related: [nf-core/three-step-pipeline]
summary: move Recipe::shared_params and the resolved_* methods out of recipe.rs
---
# Give parameter resolution its own module

## Goal

Give the step that combines a render's three inputs — decode, roll, style — one home in
code. `nf-core/three-step-pipeline` kept the stages defined by operation and made that
combination the place the three meet (design-spec §6, "Two renderings"); today it is
`Recipe::shared_params` and the `resolved_*` / `applied_*` methods spread through
`src/recipe.rs`, which also parses, merges and validates.

## Design

Known:

- Pure functions from the recipe (and the rendering's `Base`) to the stages' params, in
  their own module; `Recipe` keeps parsing, merging and validation.
- A refactor: no pixel, report or recipe change. The goldens and the drift gate must not
  move.

Open:

- Whether the report's provenance (`chain.roll`, `ResolvedSlope::base_from`) moves with
  it.
- Where `Recipe::for_frame` belongs: it resolves the frame's `roll.frames` entry, but
  `roll` and `convert` both call it before validation.

## How to Verify

- Every CI gate green, with no golden, fingerprint or snapshot change.
- `src/recipe.rs` no longer holds the resolution methods.
