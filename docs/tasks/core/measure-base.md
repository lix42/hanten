# `measure-base`, and `measure-roll` as the one-stop measurement

**Design:** [the roll workflow](../../design/roll-workflow.md) — the single source for this task's CLI shape; where they disagree, the doc wins.

## Goal

Build the design doc's measuring commands: rename `estimate` to `measure-base`; give
`measure-roll` `--unexposed`, so it measures the film base too; and have both write
their result as a recipe file, so no step between measuring and converting needs `jq`.
`measure-base` stays for a single-frame `convert` and for a roll with no unexposed
frame (decided 2026-09-28). Carrying the clamps inside the recipe (the design doc's
open question 3) is this task's, and so is `roll` applying them from the recipe.

## What is known

- **The report's `calibration` object** exists only to be extracted with `jq`
  (`cli::CalibrationFragment`); decide whether it goes once the file replaces it.
- **`nctool roll` runs `estimate`** (`scripts/analysis/nctool/roll.py`), and the
  reference build (`reserve`) has only `estimate`, so `nctool` must choose the name per
  build (see `scripts/analysis/CLAUDE.md`).
- **`measure-roll --unexposed` and `measure-base` share one base measurement** —
  one function, one report shape — or the two drift. `--unexposed` measures the whole
  frame with the reference-frame method `film-base/holder-masked-measurement` and
  `film-base/tiling-uniformity-validator` settle (today the grid).
- Moved here from `nf-core/subcommands` (2026-09-28): `estimate`'s place in the
  workflow is this task's.

## Open questions

- The design doc's open questions 3 (the per-frame clamps' shape in the recipe) and 4 (`--out` over an existing file); record answers there.
- Does `--grid` stay here or retire with `film-base/tiling-uniformity-validator`?

## How to Verify

- `measure-roll … --unexposed blank.tif --out roll.json` then `roll --params roll.json`
  renders the same pixels as today's workflow with the base from `estimate --grid` on
  the same frame **and** `measure-roll`'s `reuse.frames` passed as `roll --frames` —
  without the manifest, a clamped frame differs by design.
- `--unexposed` beside `--film-base`, or beside a `--params` stating a non-null
  `calibration.film_base`, is refused; so is the unexposed file among the frames.
- `measure-base … --out base.json` then `convert --params base.json` renders the same
  pixels as `convert --film-base <the flag the report prints>`.
- `hanten estimate` exits 2 naming `measure-base`; the `nctool` suite passes against
  both the current and the reference build.
- `docs/using-nc.md` §4 teaches the two-command workflow with no `jq`.

## Dependencies

- [The `calibration` recipe section](calibration-recipe-section.md)
- [The `roll` recipe section](../nf-calibration/roll-section.md) — the shape
  `measure-roll --out` writes
