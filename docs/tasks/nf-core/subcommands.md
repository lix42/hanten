# `roll`'s per-frame overrides under the new chain

**Design:** [the roll workflow](../../design/roll-workflow.md) — the single source for this task's CLI shape; where they disagree, the doc wins.

## Goal

Make a `roll` frame's own `params` override behave correctly on the new chain: it
resolves the same config a single `convert` would, and an override that breaks the
roll's consistency is surfaced. Settle this before
[`core/recipe-composition`](../core/recipe-composition.md) layers flags and recipes on
top of the same resolution.

*Re-scoped 2026-09-28* from "`roll`, `inspect` and `estimate` under the new chain",
after `nf-core/default-flip` left only one chain. What moved out:

- `inspect` no longer reports a `dmax` (`nf-retire/dmax-machinery`); its report is
  pre-chain facts only. Every command runs memory preflight.
- `estimate`, and the calibrate-once procedure built on it, is
  [`core/measure-base`](../core/measure-base.md).
- `nctool roll`'s calibrate step (which runs `estimate` but not `measure-roll`) is
  [`core/roll-measure-mode`](../core/roll-measure-mode.md).

## What is known

- **The overlay merges onto the serialized shared recipe** (`cli::resolve_frames`), so
  every key is present when a frame's recipe deserializes. A default that keys off a
  key's *absence* cannot fire there, and it fails on a whole roll rather than one
  frame. Check whether any `recipe_version` 2 default does; if none does, pin it with
  a test through the real path rather than restructure.
- **Roll-consistency warnings cover only the old set.** A per-frame override warns
  (`--strict`-promotable) on `calibration.film_base` and `output`. Nothing warns on
  `reconstruction.anchor` or `reconstruction.linearization`, which pin a frame on a
  different rule or slope than its roll.
- **`roll.white_stops` per frame is legitimate.** `measure-roll`'s `reuse.frames`
  gives each clamped frame its own white, so a warning on the `roll` section would
  fire on correct use. `roll.white_balance` per frame is the consistency break.
- **The error surface is a contract.** Removed flags and values exit with their
  design-spec §11 codes (largely delivered by `nf-retire`); a frame refused for any
  reason inside `roll` is recorded in its report entry, its siblings still write, and
  `roll` exits 1 — as the memory gate's exit 6 already behaves.

## Open questions

- Which new-chain keys are roll-fixed, and which are frame-local by design?
- Does a warning name the measured value it overrides, as the `film_base` one does?

## How to Verify

- A roll frame with an override resolves the same config the equivalent `convert`
  does, tested through the binary rather than the resolver.
- An override on each roll-fixed key warns and `--strict` refuses it; a
  `reuse.frames` manifest from `measure-roll` runs with no warning.
- A frame refused inside `roll` exits the roll 1 with its siblings written.
- The four CI gates pass.

## Dependencies

- [A minimal end-to-end render](minimal-end-to-end.md)
