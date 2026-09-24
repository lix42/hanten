# Hanten — nf-retire Progress Log

Execution log for the `nf-retire` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

Remove the old paths once the reference build exists: `legacy`/`custom`, the bounded display tones, the sigmoid and `simple`, the `Dmax` anchor machinery, the regional balance, and the `print.*` prefix.

Created on 2026-09-19 as part of the new-flow migration plan (`docs/nf-migration.md`).
Landed so far: **`legacy-custom`** (2026-09-23).

**What `legacy-custom` means for the rest of the epic.**

- **One implementation of the print controls is left** — `render_split`'s, past the
  ACEScg boundary. `highlight_compress` now means only the display knee, which is the
  block `display-tones` was waiting on, and `print-prefix-rename` can rename without
  renaming twice.
- **Every preset is atomic.** `output.depth` / `output_profile` / `bigtiff` are gone;
  depth comes from `OutputParams::depth()` (a function of the preset) and BigTIFF is
  always `auto`. A future destination that wants an f32 or ICC choice adds its own knob.
- **The drift gate now hashes `algo::reconstruct`** (`stages::golden::reconstructed`)
  plus the default white balance's resolved gains, which reproduced the v5 `render`
  hash exactly — the default print was a bit-exact identity. `nf-verification/fingerprints`
  still owns deciding where the new `render` hash stops.
- **Tests state their preset.** `tests/pipeline.rs` no longer injects one; a TIFF
  test names `display-p3` (u16) or `film-master` (f32), and a test that needs
  clipping uses `--display-tone reinhard`, the one tone that overshoots white.

## legacy-custom

**Status:** done
**Updated:** 2026-09-23

- 2026-09-19: created with the new-flow plan. Goal: retire `legacy` and `custom`.
- 2026-09-23: **plan.** Decisions taken with the user before starting: (1) retire the
  `--out-depth` / `--output-profile` / `--bigtiff` selectors now — once every preset is
  atomic they accept only their defaults; (2) `tests/pipeline.rs` states each preset at
  the call site rather than flipping the injection; (3) `benchmark.json` moves to
  `display-p3` / `film-master` as a holding set until `nf-verification/benchmark-set`;
  (4) keep v5 in the drift gate by hashing the reconstruction, no `pipeline_version` bump.
- 2026-09-23: **done.** Removed `OutputPreset::{Legacy, Custom}`, `is_atomic`,
  `stages::{render, render_legacy, reconstruct_and_print}` (film-master is now
  `stages::render_film_master`, whose signature carries no print parameters),
  `algo::finish_print`, `density::{render_print, soft_clip}`, `color::to_output` with
  `resolve_output_space`, `OutputSpace::{parse, ProPhoto, Custom}`, and the
  `validate_output_preset` atomicity and legacy-branch rules (the two left are
  renumbered: 1 film-master, 2 reinhard). All five names are removed-value errors (exit
  2) from flag and recipe alike; the selectors share one table,
  `cli::REMOVED_OUTPUT_SELECTORS`, so the two provenances say the same thing.
  - **Drift gate:** default print was a bit-exact identity on the golden vectors, so
    hashing `algo::reconstruct` with the `white_balance` line from
    `white_balance::resolve_print_gains` reproduced v5's `render` hash (`9c97b6954612c356`)
    and `base`. Only `recipe` moved (three output keys left the default document):
    refreshed in place to `9ca8dcca192e605a`, no bump — no default pixel moved.
  - **Goldens:** moved onto `algo::reconstruct`. The two customized ones carried a
    non-default print, so they were recaptured without it; re-applying the retired
    print arithmetic to the new bits reproduces the old captures to f32 rounding (worst
    relative error 2e-6). The three auto-WB goldens pinned the estimate on pre-matrix
    film RGB, a placement no chain has, and were deleted with ~15 print-stage unit tests.
  - **`tests/pipeline.rs`:** the injection is gone and `run_exact` merged into `run`.
    A temporary guard that panicked on any preset-less `.tif` convert found every call
    site that had relied on it (88 tests); each now names `display-p3` (or
    `film-master` / `hdr-linear-tiff` where it wanted f32). Four tests about legacy
    itself were deleted. Three clipping tests switched to `--display-tone reinhard`,
    since the shoulder never clips; the memory test's peak moved to the render phase
    (the SDR TIFF profile's measured peak).
  - **Scripts:** `real-scan-verify` recipes and `harness.sh` write `display-p3` and
    `hdr-linear-tiff`; `benchmark.json` is a `display-p3` / `film-master` holding set.
    nctool keeps *reading* `legacy`/`custom` and their encodings, because the
    reference build still writes them.
  - **Colorimetry:** `PROPHOTO` lost its one runtime consumer and stays for the
    `ADOBE_RGB` reason (nctool reads it); the lcms2-consumed set is four spaces now.
- 2026-09-23: **review round** (`/code-review high`). The one real regression: every
  sidecar and `--dump-params` recipe the previous build wrote carried the three
  retired output keys at their defaults, and the first cut refused any recipe that
  merely *contained* them — so no old recipe replayed, and the message blamed a flag
  nobody typed with a remedy that did not work. Now a key at its old default is
  dropped on load (`strip_old_default_output_selectors`, on `convert` and roll's
  per-frame override), and a non-default one is refused with "Remove the key". The
  selector table became `RemovedOutputSelector` rows looked up by key, so nothing
  depends on row order. Also: `convert_is_deterministic` runs `display-p3` again (the
  one TIFF path through the parallel lcms2 transform), and the drift gate's
  `white_balance` line is documented as echo-only — it covers no print arithmetic.

## display-tones

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: retire the `shoulder` and `none` tones.

## sigmoid-and-simple

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: retire the sigmoid and `simple`.

## dmax-machinery

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: retire the `dmax` anchor machinery.

## regional-balance

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: retire the regional balance.

## print-prefix-rename

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: rename the `print.*` prefix.

## characteristic

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: filed after the plan review. Goal: retire the `characteristic` curve path.
