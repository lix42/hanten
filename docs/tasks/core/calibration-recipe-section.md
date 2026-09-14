# The `calibration` recipe section

## Goal

Give the roll measurements their own top-level recipe section, `calibration`, holding
`film_base` and `dmax`, so that a pipeline profile is "a recipe with no `calibration`
section" and a roll calibration is "a recipe with nothing else" (design-spec §8).
`dmax` leaves `reconstruction.curve`, where it sits only because it was once a
parameter of the exponential equation. No pixel changes.

## Why it is its own task

Three tasks already assume the section and none owned creating it:
`core/base-acquisition-planner` emits `calibration.film_base` / `calibration.dmax`,
`core/profile-authoring` validates "a profile", which has no `calibration`, and
`core/recipe-composition`'s verification composes a profile with a calibration.
`core/value-domain-terminology` carried the `dmax` move as its item 3; that item now
lives here, and the terminology task keeps only its documentation and help-text work.

## What is known

- The move is a schema change on every sidecar and `--params` file. Project policy is
  a **migration error** for the old shape, never an alias (the `algorithm` /
  top-level `density` precedent).
- `dmax` is a roll calibration, not a look, which is why `cli::preset_curve` today
  carries it across when a `--preset` replaces the curve object, on the same
  `takes_dmax()` condition as the `--density-curve` merge arm. Once `dmax` is outside
  the curve that carry-across is unnecessary, and the hole it leaves today closes with
  it: a recipe frozen through a characteristic preset has no `dmax` in its curve, so
  a later `--preset sigmoid-flat` on that recipe renders off the nominal rather than
  the roll's measurement, at exit 0.
- `film_base.source` has no default and `dmax` resolves to `fixed`; moving them does
  not change either rule.
- The `recipe` fingerprint hash moves (the default recipe document changes shape) while
  `render` and `base` do not. That is the case `version::PIPELINE_FINGERPRINTS`
  sanctions refreshing in place, not a `pipeline_version` bump.
- The `characteristic` curve resolves no `dmax`. A `calibration.dmax` beside a
  characteristic curve is carried, not consumed; decide whether that is reported.

## Open questions

1. Does `curve.anchor` stay in the curve? Design-spec §8 says yes: the anchor is the
   rule for what the reference places, which is part of the look.
2. What does `nc estimate` write today, and does its output become a bare
   `calibration` object so it can be handed to `--params` unchanged?
3. Every place that resolves `dmax` by path: the recipe `Deserialize`, the merge arms,
   the `roll` planner, the report and sidecar writers, `--dump-params`, the guide's
   examples, the committed `recipes/*.json` fixtures and the review matrices.

## How to Verify

- The old shape (`reconstruction.curve.dmax`, top-level `film_base`) is rejected with a
  migration error naming the new path.
- A recipe carrying only `calibration` converts identically to the same values given
  by flags; a recipe with no `calibration` and the base from a flag converts too.
- Every existing conversion is byte-identical: `render` and `base` fingerprints
  unchanged, only `recipe` refreshed with its rationale noted in the log.
- `nc estimate` output round-trips into `--params` without hand editing.
- `docs/using-nc.md` updated by running the binary.

## Dependencies

- [Roll conversion](roll-conversion.md) — the roll planner reads both measurements.
- [Conversion versioning](conversion-versioning.md) — owns the `recipe` fingerprint
  row this refreshes.

Depended on by [recipe composition](recipe-composition.md),
[profile authoring](profile-authoring.md) and the
[base-acquisition planner](base-acquisition-planner.md).
