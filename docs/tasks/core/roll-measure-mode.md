# `roll`'s measure mode

**Design:** [the roll workflow](../../design/roll-workflow.md) — the single source for this task's CLI shape; where they disagree, the doc wins.

## Goal

Build the design doc's measure mode: `roll --measure-roll [--leader L]` runs
`measure-roll`'s code over the frames with whatever base the recipe supplies, then
converts them; `--unexposed U` supplies the base by measuring it and implies the mode;
and `--save-recipe PATH` writes the resolved run as one replayable recipe. `roll`'s
requirements and defaults do not change.

## What is known

- **`nctool roll` is an internal one-shot** (moved here from `nf-core/subcommands`,
  2026-09-28): it calibrates with `measure-base` (`estimate` on the reference build)
  alone, never `measure-roll`, then renders.
  Once measure mode exists, `nctool roll` uses it on builds that have it and keeps its
  own steps for the reference build (`scripts/analysis/CLAUDE.md`).
- Every command runs memory preflight; measure mode peaks at
  the larger of `measure-roll`'s and the destination's, and that is measured, not
  assumed.

## Open questions

The design doc's 7 (`--strict` across phases) and 9 (`--frames` in measure mode);
record answers there.

## How to Verify

- On a real roll, `roll --unexposed … --leader …` and `roll --params base.json
  --measure-roll --leader …` are each byte-identical to `measure-roll` then `roll` run
  by hand over the same inputs.
- In measure mode a non-null `roll` value or a `--roll-*` flag is refused, and so is
  `--unexposed` beside a stated base; `--leader` outside measure mode is refused; a
  recipe with a base and no `roll` section renders as today.
- `--save-recipe` replays byte-identically through `roll --params`; without it, no
  recipe file is written.
- A measurement failure writes no image and exits with the measurement's code.

## Dependencies

- [`measure-base`](measure-base.md) — `measure-roll --unexposed`, which this mode runs
- [Layered recipe composition](recipe-composition.md) — `roll`'s flags and layers
