---
title: "`roll`'s measure mode"
status: done
deps: [core/measure-base, core/recipe-composition]
tags: [post-mvp]
summary: "`roll --unexposed <file|auto> [--leader L]` measures the base and the roll, then converts"
done: 2026-10-10
---
# `roll`'s measure mode

> **From TASKS.md** (migrated 2026-10-14): `roll --unexposed <file|auto> [--leader L]` measures the base and the roll, then converts; `--measure-roll` does the same over a base the recipe already states. `roll`'s requirements and defaults are unchanged. Opt-in `--save-recipe` writes the resolved run for re-rendering the same roll. **Done 2026-10-10**

**Design:** [the roll workflow](../../design/roll-workflow.md) — the single source for this task's CLI shape; where they disagree, the doc wins.

## Goal

Build the design doc's measure mode: `roll --unexposed U [--leader L]` measures the
base, then runs `measure-roll`'s code over the frames, then converts them;
`--measure-roll` enters the mode over a base the recipe already states; and `--save-recipe PATH` writes the resolved run as one replayable recipe. `roll`'s
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
- **Since 2026-09-28** the `roll` section gained `dark_stops` (measured) and four
  render-time switches (`small_lift`, `thin_lift`, `midtone_neutral`,
  `neutral_balance`); `measure-roll` gained `auto` references and its own
  measurement-time `--midtone-neutral AUTO|ON|OFF`, `--no-small-lift`, `--no-thin-lift`;
  `convert --dump-params` became `--save-recipe`. Decided 2026-10-09 (user): the
  switches keep `roll`'s render meaning in measure mode and the measurement runs at
  `measure-roll`'s defaults; the report embeds the measurement; design-doc questions 7
  and 9 are answered there.

## How to Verify

- On a real roll, `roll --unexposed … --leader …` and `roll --params base.json
  --measure-roll --leader …` are each byte-identical to `measure-roll` then `roll` run
  by hand over the same inputs.
- In measure mode a measured `roll` value or a `--roll-*` flag is refused, and so is
  `--unexposed` beside a stated base, and `--frames`; `--leader` outside measure mode is
  refused; a recipe with a base and no `roll` section renders as today.
- `--save-recipe` replays byte-identically through `roll --params`; without it, no
  recipe file is written.
- A measurement failure writes no image and exits with the measurement's code.

## Dependencies

- [`measure-base`](measure-base.md) — `measure-roll --unexposed`, which this mode runs
- [Layered recipe composition](recipe-composition.md) — `roll`'s flags and layers
