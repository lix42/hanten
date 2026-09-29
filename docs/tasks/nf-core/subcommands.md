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
- **Roll-consistency warnings covered only the old set** (before this task). A per-frame override warned
  (`--strict`-promotable) on `calibration.film_base` and `output`. Nothing warned on
  `reconstruction.anchor` or `reconstruction.linearization`, which pin a frame on a
  different rule or slope than its roll.
- **`roll.white_stops` per frame is legitimate.** `measure-roll --out` gives each
  clamped frame its own white (the recipe's `roll.frames`, `core/measure-base`), and a
  manifest may state one, so a warning on the `roll` section would fire on correct use.
  `roll.white_balance` per frame is the consistency break.
- **The error surface is a contract.** Removed flags and values exit with their
  design-spec §11 codes (largely delivered by `nf-retire`). A bad override is a config
  error: it refuses the whole roll at exit 2 before any frame is written. A frame
  refused while converting (decode, memory gate, content) is recorded in its report
  entry, its siblings still write, and `roll` exits 1.

## Decisions (2026-09-28, user)

- **Roll-wide:** `calibration.film_base`, `roll.white_balance`, every
  `reconstruction` key, `rendering`, and the resolved destination. Everything else is
  frame-local (`cli::ROLL_WIDE`; a test holds every recipe key to one side).
- **A warning fires when the frame's resolved value differs from the shared one**, and
  names both. A restatement is silent — it replaces the old key-presence probe.
- **No `recipe_version` 2 default keys off a key's absence** (unset values serialize as
  `null`, unset destination axes are left out and derive from the frame's own
  `rendering`), so nothing was restructured; the binary test
  `a_roll_frame_resolves_as_the_equivalent_convert_and_warns_only_on_roll_wide_values`
  pins it.
- **Config errors stay up front** (exit 2); only a frame refused while converting is
  per-frame.

## How to Verify

- A roll frame with an override resolves the same config the equivalent `convert`
  does, tested through the binary rather than the resolver.
- An override on each roll-fixed key warns and `--strict` refuses it; a
  `reuse.frames` manifest from `measure-roll` runs with no warning.
- A frame refused while converting exits the roll 1 with its siblings written.
- The four CI gates pass.

## Dependencies

- [A minimal end-to-end render](minimal-end-to-end.md)
