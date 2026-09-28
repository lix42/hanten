# The roll's measurements as their own recipe section

## Goal

Carry what `hanten measure-roll` measures in a `roll` recipe section of its own, apart
from the style knobs, so a rendering can apply it or leave it out
(`nf-destinations/direct-preset`). Today the gains land in
`scene_correction.white_balance` and the contrast in `look.contrast`, where a measured
value cannot be told from a chosen one.

## Design

The design is `docs/design-update.md`, Part 2, "Two renderings" (2026-09-27). What is
known:

- **Two values**: the white-balance gains `measure-roll` reports, and the roll's white
  in scene stops above mid-grey. The white is stored as the **measurement**; the
  rendering turns it into a contrast with `pipeline::roll_white::contrast_for`, so the
  section holds no contrast value and stays out of `look.contrast`'s unit question
  (`nf-look/contrast-definition`).
- **A frame clamped to the cap** gets its own white through `roll --frames`, as
  `roll-white-rule`'s manifest does today with `look.contrast`.
- **Every recipe key is a flag**: a flag per value, a `recipe::merge` arm with a merge
  test, a value rule, and a row in `flow`'s new-flow-only table.
- **`measure-roll`'s reuse output** (flag, recipe fragment, `--frames` manifest) writes
  `roll` instead of `scene_correction` / `look`.
- **Applied the way `default` will apply it**: the roll's gains are multiplied by
  `scene_correction.white_balance`, and the contrast from the white stands unless
  `look.contrast` is stated. Until `--rendering` exists this is the only behaviour, so
  a recipe with a `roll` section renders as the same values in the old keys did.
- **No key retires.** A recipe that carries `measure-roll`'s old output in
  `scene_correction` and `look` still renders as before; those keys are simply style now.

Open:

- The flag names.
- Whether the report states the contrast derived from the white, and its source.

## How to Verify

- `measure-roll`'s new fragment, replayed through `convert`, renders byte-identically to
  the same values stated in the old keys.
- Merge tests for each key, including a roll's per-frame override of the white.
- `docs/using-nc.md` and design-spec §9 updated, checked by running the binary.

## Dependencies

- [`measure-roll` places the roll's white](roll-white-rule.md) — the values this section
  carries
