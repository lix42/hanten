# Retire `legacy` and `custom`

## Goal

Remove the `legacy` and `custom` output presets, and with them the second
implementation of the print controls. This is the retirement every later one
waits on.

## Design

- **Two implementations of one idea.** `density::render_print` runs white
  balance, exposure and black point on film RGB *before* the 3×3, while the
  display presets run the same controls on ACEScg. Retiring legacy is what leaves
  one.
- **Decide by deletion.** `color::to_output`'s only non-test caller is the legacy
  render, so ProPhoto output, arbitrary `--output-profile <icc>` paths and the
  rendered float TIFF go with it. Each is an ability the new code may add on its
  merits — Adobe RGB is a must-have and has its own task, f32 is good to have,
  ProPhoto and the ICC path are undecided — not a reason to keep the branch
  alive.
- **`custom` goes with it**: it is the same legacy branch plus the
  depth/profile/container selectors, which is why it is the one named preset that
  is not atomic. Its removal simplifies `OutputPreset::is_atomic()`'s three call
  sites.
- **The legacy-measuring coverage is not preserved.** `tests/pipeline.rs`'s
  `run()` injects `--output-preset legacy` into ~87 tests that predate the
  gain-map default. Rewrite the ones that still state a rule about the *new*
  chain; delete the rest rather than porting them mechanically — a gate that
  measures the design being replaced only pins it.
- Removed names get a removed-value error, the `--algorithm` precedent: no
  aliases, and a recipe naming one fails with a migration message.

## How to Verify

- `OutputPreset::ALL`, `parse`, its rustdoc and `--help`'s text all agree that
  the names are gone — those four have gone stale together twice.
- A recipe or flag naming either preset exits with the documented code and a
  message pointing at the replacement.
- The tagged reference build still renders the old behaviour, which is the only
  place it now lives.

## Dependencies

- [A minimal end-to-end render](../nf-core/minimal-end-to-end.md)
- [The frozen reference build](../nf-verification/reference-snapshot.md)
