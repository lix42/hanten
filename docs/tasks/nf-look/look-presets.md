# Re-express the `--preset` bundles

## Goal

Decide what `--preset` means under the new flow. The five shipped bundles each
name a reconstruction curve that is retiring and carry an exposure calibrated to
the old chain, so none of them survives unchanged; the likely answer is that
`--preset` becomes a *look* preset.

## Design

- Today a preset sets four knobs — curve, `density.scale`, `print_exposure`,
  `display_tone` (`cli::ConversionPreset`, five names). Under the new flow the
  decode is fixed and stock-agnostic, so three of those either no longer vary or
  belong to another stage. What carries over is the *mechanism*: a CLI-only
  expansion with no recipe key, sitting `defaults < params < preset < flags`.
- **The numbers do not carry.** Each preset's exposure was solved so its frames
  land at a common mid-grey under the old chain; with a new anchor convention and
  a new fit range those solves are meaningless, and re-deriving them is
  calibration work.
- **A look-only preset is atomic in a way the old bundles were not**: it cannot
  reach into the decode, which removes their special-case handling around
  `film-master` (which still refuses a look at all — see the stage task).
- **The `characteristic-*` names have no referent** once `characteristic` leaves
  reconstruction. [stock-data-home](stock-data-home.md) answered what returns: an
  optional per-stock normalization in the look, planned but unscheduled, with its own
  spelling — not the preset names.
- **It must work on a roll.** `docs/tasks/core/recipe-composition.md` adds
  `--preset` to `roll`'s override surface — the only way to apply a look to a roll
  without authoring a file. Whatever this becomes keeps that precedence position.

## Open questions

- How many presets, what they are named after, and whether the default is "no
  look" or a named one.
- Whether a preset name is provenance in the report only, or a recipe key that
  re-expands — the old task chose CLI-only; re-check the reasoning, don't inherit
  it.

## How to Verify

- Every shipped name either resolves to look knobs only, or errors as a removed
  flag value naming what replaced it — no name silently changes meaning.
- The expansion is visible to the user (whatever succeeds `--dump-params`), and a
  recipe naming a preset is still rejected if the CLI-only rule is kept.
- A preset applied to a roll reaches every frame, and a per-frame flag still wins.

## Dependencies

- [The print-contrast knob](contrast.md)
- [A per-channel grade with a mid-grey pivot](per-channel-grade.md)
