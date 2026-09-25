# The destination set

## Goal

Settle which destinations a render can go to under the new flow, how they are
selected alongside `--new-flow`, and how each one's file suffix is decided. This is
the list and the selection rules; the individual destinations are separate tasks.

## Design

What is known:

- **Ten preset names ship today**: `legacy` and `custom` retired with the print path
  (`nf-retire/legacy-custom`, 2026-09-23), taking `to_output`, ProPhoto, arbitrary ICC
  and the `--out-depth` / `--output-profile` / `--bigtiff` selectors with them.
  `film-master` is not a rendering destination at all — it is the reconstruction
  output, and runs no rendering stage.
- **Two abilities move across from legacy**: Adobe RGB output is a must-have (it is
  was reachable only through an ICC path on legacy, so today it is not reachable at
  all), and a rendered float TIFF is good to have (`hdr-linear-tiff` is the current one). Retiring a path is a list of abilities to re-add, not a loss.
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
- **A new-flow gain-map destination consumes `chain::render_pair` and
  `pipeline::gain_ratio`** (`nf-display-stages/branch-contract`, 2026-09-24): the
  pair gives the SDR base and the HDR alternate from one look, and `gain_ratio`
  gives the per-channel gains. Their rules: the gain is ratioed against the base
  **as stored** (clamped to `[0, 1]`), never the unclamped rendition; and a flat map
  (`GainRange::flat`) is reported rather than shipped silently.
- **The gain-map destination must clamp HDR to its peak and count what it clamps**,
  as the single-rendition encoders do. Fit range sets no hard ceiling (decided
  2026-09-24) because the encoder clamps and counts above-peak samples, but
  `gain_ratio::between` clamps the HDR rendition only to `>= 0`: a sample above the
  peak (measured up to about `2 P` at headroom 2) would otherwise become a larger
  gain, uncounted.

Open:

- **Is a destination a name, or a small product of selectors?** Every preset is
  atomic since `custom` and `--out-depth` retired, and the presence-vs-value asymmetry
  `--out-depth` needed is recorded in `cli::validate_convert`'s docs. A gamut selector on an otherwise
  atomic destination would reopen that; a name per combination reopens the thirteenth-name problem.
- **What does a destination mean while `--new-flow` is off?** Either the names are
  new and only exist under the flag, or they are the same names resolving a
  different chain — which changes what a recipe means between builds.
- Whether HDR destinations stay first-class before the default moves.

## How to Verify

- Every destination in the list resolves end to end, states its suffixes, and is
  covered by a parse diagnostic generated from the same list (the `OutputPreset::ALL`
  precedent, so the name list and the help text cannot desynchronize).
- `hanten roll` derives a name for each; `docs/using-nc.md` updated by running the binary.
- A look control has landed (`nf-look/path-to-white`): `film-master` with a look the
  user set refuses, naming the look rather than a downstream knob — **one** rule keyed
  on whether the destination runs a look, never one per look knob (`nf-look/stage`).
  Key it on `LookSection::asks_for_a_look` — neither the default nor empty. Not "any
  look at all": highlight desaturation is on by default, so that would refuse every
  default recipe. Not "not the default" either: an empty look
  (`--contrast 1 --highlight-desaturation 0`) renders exactly what `film-master` does, and refusing that identity kills the
  flags-win reset.
- One recipe drives the same look through every destination that runs one, and the
  report names the look stage on each, empty or not.

## Dependencies

- [The SDR/HDR branch contract](../nf-display-stages/branch-contract.md)
- [Derive the output suffix from the resolved preset](../output/output-path-suffix.md)
