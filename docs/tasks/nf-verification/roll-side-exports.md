# Per-frame side exports from `roll`

## Goal

Let `hanten roll` write each frame's side exports, the pre-matrix film RGB
(`--export-film-rgb`) and the IR plane (`--export-ir`), next to that frame's output
in `--out-dir`. Today `roll` refuses both, because one path cannot serve every frame,
so a roll has to be exported one `convert` at a time. The `scale` calibration is
measured across a roll's frames, which is where a per-frame export pays off.

## Design

- **Known:** a roll frame is `convert_frame` with the roll's recipe, so the export
  itself exists (`film-rgb-export` stages it in `render_frame`). What is missing is
  naming and guarding a path per frame. `--export-film-rgb` is an operational flag on
  `ConvertArgs`. `--export-ir` is a recipe key (`input.export_ir`) that
  `reject_roll_unsupported` refuses in roll mode.
- **Open: the shape on `roll`.** A switch that derives names (`<stem>_film-rgb.tiff`
  beside `<stem>_positive.<ext>`), or a directory, or something else. `convert`'s
  path-valued form does not carry over.
- **Open: `--export-ir` as a recipe key.** A path in a shared recipe is meaningless
  across frames. Decide whether roll's IR export is a flag like the film RGB one, and
  what happens to `input.export_ir` in a roll recipe.
- **Open: an explicit `--frames` manifest `output`.** Whether the export's name is
  derived from that path, and how it is guarded.
- **Open: collisions.** Each derived path must be checked against every frame's
  outputs and the other exports, case-insensitively (`ensure_write_targets_distinct`).

## How to Verify

- A roll over several frames with the export on writes one export per frame, and
  each one is byte-identical to that frame's `convert --export-film-rgb`.
- A derived name that collides with another frame's output is refused before any
  frame renders.
- `docs/using-nc.md` states the new roll behaviour, verified against the binary.

## Dependencies

- [Export the pre-matrix film RGB](film-rgb-export.md)
