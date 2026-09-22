# The destination set

## Goal

Settle which destinations a render can go to under the new flow, how they are
selected alongside `--new-flow`, and how each one's file suffix is decided. This is
the list and the selection rules; the individual destinations are separate tasks.

## Design

What is known:

- **Twelve preset names ship today** and two (`legacy`, `custom`) retire with the
  print path, taking `to_output`, ProPhoto and arbitrary ICC with them.
  `film-master` is not a rendering destination at all — it is the reconstruction
  output, and runs no rendering stage.
- **Two abilities move across from legacy**: Adobe RGB output is a must-have (it is
  reachable today only through an ICC path on legacy), and a rendered float TIFF is
  good to have. Retiring a path is a list of abilities to re-add, not a loss.
- **`--new-flow` is scaffolding, not a feature** (`docs/nf-migration.md`): CLI-only,
  never a recipe key, and it dies when the default flips — so destination selection
  must not be built on it.
- **Suffix derivation is carried, not redesigned.**
  [`output/output-path-suffix`](../output/output-path-suffix.md) shipped it
  (2026-09-22); this task states each destination's accepted spellings. The seam it
  left is `cli::container_for` — the **only** preset-shaped step, with the accepted
  set and the supplied spelling both hanging off `Container`. So the open question
  below (name vs product of selectors) changes that one function under either
  answer, and nothing about suffixes needs re-deciding. Keep it an exhaustive match
  so a moved `OutputPreset` fails to compile.
- **`nctool` needs one row per destination in two lookup tables** — the only
  tooling change the migration owes.

Open:

- **Is a destination a name, or a small product of selectors?** Today `custom` is
  the one non-atomic preset and the asymmetry around `--out-depth` exists because
  of it (CLAUDE.md). A gamut selector on an otherwise atomic destination would
  reopen that; a name per combination reopens the thirteenth-name problem.
- **What does a destination mean while `--new-flow` is off?** Either the names are
  new and only exist under the flag, or they are the same names resolving a
  different chain — which changes what a recipe means between builds.
- Whether HDR destinations stay first-class before the default moves.

## How to Verify

- Every destination in the list resolves end to end, states its suffixes, and is
  covered by a parse diagnostic generated from the same list (the `OutputPreset::ALL`
  precedent, so the name list and the help text cannot desynchronize).
- `hanten roll` derives a name for each; `docs/using-nc.md` updated by running the binary.

## Dependencies

- [The SDR/HDR branch contract](../nf-display-stages/branch-contract.md)
- [Derive the output suffix from the resolved preset](../output/output-path-suffix.md)
