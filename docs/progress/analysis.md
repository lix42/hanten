# Negative Converter — analysis Progress Log

Execution log for the `analysis` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status (the checkboxes);
this file is the narrative beside it.

One `##` section per task in this epic, named by the bare task name (the part
after the `/`). Read this whole file before starting a task in this epic, and
read other epics' `Epic summary` sections when you depend on them. Append
entries — don't rewrite earlier ones.

## Epic summary

What other epics need to know about `analysis`:

- **This epic verifies the pipeline; it is not part of it.** Everything lives in
  `scripts/`, and the hard invariant is that **only derived numbers and
  downscaled thumbnails leave the tools** — never sample pixels into context.
  Metadata comes from `nc inspect`; bytes are streamed only to hash.
- **`nctool metrics` reads output pixels (2026-09-02), and is the toolkit's only
  command that does.** Every other number here comes from `nc`'s own JSON report
  and therefore exists only for nc outputs; `metrics` measures any producer's
  image — NLP, SmartConvert, a hand-edited export — on the same footing. It is
  also the only command that is not stdlib-only (`numpy`, `tifffile`, via
  `scripts/analysis/requirements.txt`, plus `Pillow` for JPEG; CI installs them into
  a venv and sets `NCTOOL_REQUIRE_DEPS=1`). It still emits derived numbers only. `metrics image`
  measures one file; `metrics roll` measures a converted roll and rolls the scalars
  up into a spread table; `metrics table` re-renders that as Markdown. Four facts
  other epics may want: an input's colour space must be **declared**, never
  inferred (except from a run's frozen recipe, which is provenance); a full-frame
  measurement of an uncropped scan measures the **film holder** as much as the
  picture (it renders to white — excluding it moved one frame's median by 3.1
  stops); `cast_by_tone_band` is the **crossover** detector, the one colour number
  a negative conversion turns on; and a roll's spread is **not attributable** to
  the calibration, because scene content is mixed into it.
- **Comparing renders by eye is one command (2026-09-12).** `nctool review generate
  <matrix.json>` renders every (frame x config) cell a matrix names, writes each rendition's
  `nctool metrics` record beside it, and emits the `review.json` that `tools/review-app`
  reads; `scripts/preset-review/presets.matrix.json` is the worked example. Two things other
  epics will care about: the matrix is **data**, so comparing a new configuration is an edit
  to a JSON file rather than to any script, and a review set now carries its **measurements**,
  which the app draws as tone and cast charts under each picture. It needs `../nc-assets` and
  the metrics venv, so it is not in CI, and its output goes to a throwaway directory outside
  the repo — the frames are the user's own photographs.
- **The tone bands are cut in CIELAB lightness, and the record is `schema_version`
  2 (2026-09-10).** Edges every 15 L\* to 75, then diffuse white (L\* 100), then an
  overflow band above it — `deep_shadow, shadow, low_mid, mid, high_mid,
  highlight, above_diffuse_white`. They were even in *stops* through schema 1
  (-4 / -2 / +2 / diffuse white), which put a median 83% of a real frame in `mid`
  alone. **Anyone quoting a band share from before 2026-09-10 is quoting the old
  definition**; `metrics table` refuses a schema-1 record rather than rendering it
  under the new labels. The cut is stated inside every record (`record.bands`), so
  an artifact carries its own definition. `cast_by_tone_band` entries now carry
  `pixels` and a `sparse` flag (under 0.1% of the region), and the rollup withholds
  `crossover_*` when either contributing band is sparse.
- **`tone.histogram` is the only list-valued field in any of this toolkit's
  records.** Four series — luminance and each of R/G/B — binned one count per L\*
  unit over **L\* 0..200**, i.e. to twice diffuse white, with `above_range` past
  that and separate counters for samples with no lightness. Diffuse white sits on
  the bin-100 boundary and the record names `mid_grey_bin` / `diffuse_white_bin`.
  The `luminance` series uses the **declared space's own luma weighting**, the same
  one `tone.percentiles_stops` is built from, so the two cannot disagree — and it
  is emitted rather than derived at draw time because luma is a weighted sum of
  linear channel values and is not recoverable from three per-channel histograms.
  It is there for `analysis/metrics-visualization` to draw; the band edges fall
  exactly on bin edges, so bands and bars share one axis. It streams, so it costs
  ~0.6 s at 18.7 MP and no measurable memory; a record is ~8 KB.
- **For anyone consuming nc's ProPhoto output:** `color::build_profile` writes a
  **pure 1.8** power law, omitting the ROMM linear toe the standard specifies. A
  decoder applying the toe disagrees with nc's own pixels below encoded 0.03125 —
  1.3 stops out at 0.01, in exactly the samples deep-shadow statistics are made of.
  `metrics` therefore carries two ProPhoto spaces and maps nc's output to the pure
  one.
- **Real-scan core verification is done (2026-07-22/23)** across five rolls; the
  write-up is [`docs/reports/real-scan-verification.md`](../reports/real-scan-verification.md)
  and the rerunnable harness plus frozen recipes are under
  `scripts/real-scan-verify/`. (The task section below still mirrors an old
  `not started` status — `TASKS.md` is authoritative and marks it `[x]`.)
- **Note:** the execution record for `real-scan-verification` is in
  [`_unassigned.md`](_unassigned.md) — in the flat log it was nested under the
  `color-management planning` heading, so the epic split carried it there
  verbatim. Read it there.
- **The numbers other epics are waiting on:** all assets are HDRi with IR;
  standard frame 5184×3599 ≈ 18.66 MP; measured **peak ~930 MiB @ 18.66 MP
  (~50 MiB/MP)**, ~1.6 s wall — about 1.5× the design's model, which omits the
  carried IR plane and the `to_output` clone. That is the STEP 0 input for
  `io/streaming-tiled-io`: `io/memory-preflight` is required, streaming is a
  conditional GO pending a post-preflight re-measure.
- **Also found:** `--auto-base` fails loudly on every real frame (correct, given
  the holder layout — use the measured-reference workflow); u16 output clips
  4.8–10.3% high by default, routed to the display-output roadmap; float output is
  byte-lossless; determinism is byte-identical.
- **Assets live in a shared Google Drive folder**, reached through a
  **machine-local, uncommitted symlink** `../nc-assets`. The tracked inventory is
  `manifest.json` **at the assets root**, with paths relative to its own directory
  (no `asset_root` field), so it is machine-portable. sha256 is recorded for
  irreplaceable data and omitted for regenerable nc outputs.
- **`python -m nctool manifest {generate,validate,roles}`** is the entry point
  (stdlib only; needs `scripts/analysis` on `PYTHONPATH`).
  `scripts/analysis/generate_manifest.py` is now a thin shim.
  `generate` is idempotent — a re-run must stay byte-identical. `validate`
  **reports only, never deletes**, and exits 0 clean / 1 discrepancies / 2
  operational. The harness's roll list comes from `manifest roles`, not a
  hard-coded array.
- **`python -m nctool compare {run,diff}`** was added by
  `core/conversion-versioning` (logged in `docs/progress/core.md`): it converts the
  fixed benchmark set in `scripts/analysis/benchmark.json` under one `nc` build and
  diffs two builds keyed on `pipeline_version` + commit. It **reuses this epic's
  inventory** — a benchmark case names a roll + frame *stem* and resolves its path
  and `sha256` through `manifest.json`, so there is still exactly one asset
  inventory — and reads only derived numbers (the report's `output_stats` / `loss`
  and the telemetry record's timings), never pixels. It records the digest of the
  bytes it actually converted (`input_sha256` + `checksums: verified|computed|skipped`)
  so a comparison's input identity is provable from the artifact, not just from the
  exit code.
  **`compare`'s exit codes deliberately differ from `manifest`'s above:** `0` = the
  comparison ran and its verdict (identical or differing) is the report — a
  discrepancy between two *different* builds is the normal answer here, not a fault;
  `1` = the comparison failed or proved a broken invariant (a case would not convert,
  input checksum drift, cases disagreeing about the build, or one build producing two
  different results); `2` = operational/usage. The determinism claim in particular is
  **precondition-guarded** — it fires only once every *other* explanation for the
  difference is ruled out (clean pinned source, same frame set, same input digests with
  no skipped checksum, same output depth, same per-frame `params_hash`); a failed
  precondition is rc 0 plus a `determinism_check_blocked` note, never an accusation.
  Documented in `compare.py`'s module docstring and `determinism_blockers`.
- **NLP comparison is global-metrics + side-by-side thumbnails, no registration**
  — NLP outputs are cropped and differently sized, aligned only by manifest
  `source_frame` identity.
- **Open question for the user:** the committed `recipes/*.hdr.json` key order
  lags the current harness `jq` (values identical); a `freeze` re-run will
  reorder them.
- **The analysis stdlib suite is CI-gated on Linux and macOS (2026-08-11).** It
  includes a hermetic real-binary `freeze` → `convert` harness test plus a fake
  successful-wrong-container regression. The Drive-backed verification matrix
  remains manual; CI protects its CLI/recipe/container plumbing.


## real-scan-verification
**Status:** not started
**Updated:** 2026-07-21

- Goal: run the verification matrix (inspect/estimate/convert/IR/determinism/
  resources) against the full-size real scans once the user prepares the assets;
  record results here, file follow-up tasks for defects.
- 2026-07-21: Narrowed this to the current TIFF pipeline so full-size resource
  measurements can run before the HDR/display roadmap and can inform the
  `streaming-tiled-io` go/no-go. Final preset and cross-device checks moved to
  `display-output-acceptance`.
- 2026-07-27: Epic migration — the actual execution record for this task is in
  [`_unassigned.md`](_unassigned.md) (`### Real-scan core verification — executed
  2026-07-22`); it was nested under another heading in the flat log, so the split
  carried it there verbatim rather than into this section.


## display-output-acceptance
**Status:** not started
**Updated:** 2026-07-23

- 2026-07-23: Removed calibration from the dependency and acceptance matrix.
  Acceptance now verifies faithful preservation of NC's intended film rendering,
  cross-encoding consistency, tone/gamut behavior, metadata, determinism, and
  viewer interoperability rather than agreement with a physical scene.
- 2026-07-23: Made acceptance reproducible with a versioned golden manifest,
  canonical pre-encode buffers, independent decode-back oracles, quantitative
  bounds for float/SDR/PQ/HLG/gain-map outputs, normalized metadata comparison,
  and a separate binary manual-viewer interoperability rubric.
- 2026-07-23: Refined PQ/HLG acceptance to a bit-depth/transfer-derived
  independent quantization oracle (half-code lossless or spike-approved one-code
  codec allowance) over observable stored codes; pre-quantization arithmetic is
  not asserted by the black-box acceptance harness. Pinned
  cross-encoding exposure/reference-white normalization, D65 CIELAB,
  Sharma–Wu–Dalal CIEDE2000 parameters, and CIE 1976 u'v' formulas.

- 2026-07-21: Split final display/HDR acceptance from core real-scan verification.
  This task waits for output presets and reuses the verified full-size assets to
  check the gain-map default, explicit presets, metadata, deterministic encoder
  contracts, and Apple/non-Apple aware plus SDR-fallback readers.
- 2026-07-21: Added calibrated-characterization acceptance as a real dependency.
  The matrix exercises both a matching measured artifact and the explicitly
  warned/reported provisional fallback; output preset implementation itself stays
  independent of offline calibration.
- 2026-07-21: Acceptance now distinguishes a compatible measured artifact, the
  internally valid but provisional assumed-source fallback, and the untagged
  identity-device diagnostic rejected by named presets. Scene-master acceptance
  also checks fixed-Dmax cross-frame exposure preservation.


## conversion-analysis-tooling
**Status:** done (spike)
**Updated:** 2026-07-23

- Goal: decide scope/structure to grow `real-scan-verify` into a reusable
  conversion-analysis toolkit (asset manifest, image-library analysis, NLP-vs-nc
  comparison). Spike deliverable = design note + concretely-scoped child tasks.
- **Research.** Confirmed the current state: `harness.sh` drives `nc` with a
  hard-coded `ROLLS` array; **all quantitative numbers today come from nc's own
  JSON reports** (clip %, Dmin/Dmax) — the numpy/tifffile + ImageMagick analysis
  the task mentions was ad-hoc and **not committed anywhere**, so the
  image-library layer is net-new. Assets (`../nc-assets`, ~11 GB rolls + 6.2 GB
  converted) match the user's three categories: experiments (`48/64bit-*`,
  `samples/`), rolls (5, each unexposed+leader+real), converted (`V0` = the
  v0-baseline set; `2026-07-22` = harness output). **numpy/tifffile/Pillow are not
  installed** (system Python 3.14) → toolkit needs its own venv.
- **Decisions (with the user, via interview):**
  - Tooling → **Python package** `scripts/analysis/nctool/`; single entry point;
    subsumes `real-scan-verify` (`harness.sh` retires/shims).
  - Asset root → **configurable, local for now**; relative paths + portable
    checksums so a later Drive switch is one line. Drive = deferred task.
  - Manifest → **JSON, rolls + converted, experiments excluded**; roles
    (`unexposed|leader|real`) human-seeded, derived facts generated from
    `nc inspect`; replaces `ROLLS`. `manifest validate` (orphans/missing/drift) is
    the cleanup surfacing mechanism (reports, never deletes).
  - NLP → **global metrics + side-by-side thumbnails, no registration**; align by
    manifest `source_frame` identity.
- **Invariant preserved:** only derived numbers (JSON) + downscaled thumbnails
  leave the tools; full-res pixels read one-at-a-time, never surfaced.
- **Split into 4 child tasks:** `asset-manifest` → `conversion-metrics` →
  `nlp-comparison`; `asset-manifest` → `drive-asset-migration` (deferred). Graph
  wired in TASKS.md.
- **Cleanup done:** removed 4 stray `.DS_Store` from `../nc-assets`.
  `converted/V0/` kept (v0-baseline artifacts for `conversion-versioning`).
- **Next:** `asset-manifest` is the unblocked starting task.

---

**Update 2026-07-24 — assets moved to Google Drive + reorg + first manifest.**

- User relocated nc-assets from local `../nc-assets` to
  `…/GoogleDrive-devlix42@gmail.com/My Drive/temp/nc-assets` (12 GB) for
  multi-machine work, and added: `samples/largest.tif` (10368×7200 = **74.6 MP**,
  HDRi w/ IR — the ~4× perf worst-case the memory report lacked) and an NLP set
  (`NLP converted/`) for a new roll `Portra160-7-22` (renamed `Portra160-2026-07-22`
  in the reorg below).
- **Reorg (full category regroup):** `rolls/{Ektar,phoenix,Portra160,
  Portra160-2026-07-22,Portra400,Portra400-leica-flaw}`, `samples/`
  (`largest.tif` + `icc/`), `converted/{nc/{2026-07-22,V0},nlp/2026-07-23}`.
  Dropped the 48/64bit experiment fixtures (repo tests use committed
  `tests/fixtures/`, not these); kept `converted/nc/V0` (v0-baseline). Cleaned
  `.DS_Store`.
- **Manifest rethink:** lives **at the assets root** (`manifest.json`) with paths
  **relative to its own dir** → no `asset_root` field, machine-portable; scope
  broadened to a **full inventory** (rolls+roles, samples, converted nc+nlp).
  sha256 for irreplaceable data (rolls/samples/NLP/V0); omitted for regenerable
  nc/2026-07-22 outputs. `source_frame` links + `coverage_gaps`.
- **Generated** `manifest.json` (throwaway `nc inspect`-driven Python script;
  formalized later by `asset-manifest`'s `manifest generate`). `nc inspect`
  corrected exiftool: all frames incl. largest.tif are **hdri w/ IR**. NLP outputs
  are **4406×2930 32-bit float, cropped** (validates no-registration); frames
  1096/1097 have no NLP output (coverage gap).
- Task docs synced (`asset-manifest`, `drive-asset-migration` [now in-progress,
  not deferred], spike outcome, `nlp-comparison`). **Open:** repo `../nc-assets`
  convention (recommend machine-local symlink → Drive) — CLAUDE.md/harness still
  say `../nc-assets`.

**Update 2026-07-24 (cont.) — symlink bridge + committed manifest tooling + skill.**

- Created machine-local symlink `~/src/nc/nc-assets → <Drive>/temp/nc-assets` so
  the repo's `../nc-assets` convention (CLAUDE.md, harness `A=../nc-assets`,
  reports) keeps working unchanged across worktrees; not committed (machine-local).
- `scripts/analysis/generate_manifest.py` — reusable, update-aware, stdlib
  generator (locates `nc`/exiftool; preserves human fields role/stock/kind/note +
  bucket regenerable/nc_version/recipe_dir; recomputes sha256 every run by
  default for source-of-truth integrity, opt-in size-based reuse via
  `--reuse-hash`). **Idempotent** (byte-identical re-run). Reproduces the live
  manifest and adds `megapixels` on converted outputs.
- `scripts/analysis/manifest.sample.json` — committed trimmed schema reference
  (every shape; real values; ~7 KB). Update only on **schema** change.
- `asset-manifest` **skill** (`.agents/skills/asset-manifest/` + `.claude/skills/`
  symlink) — when/how to regenerate, invariants (no pixels; roles preserved),
  layout the scanner expects.
- The live `manifest.json` stays in the Drive folder (not committed). `asset-manifest`
  task doc updated to point at these precursors; remaining task work = fold into
  `nctool` + add `validate` (orphans/missing/drift).


## asset-manifest
**Status:** implemented (uncommitted, in worktree `feat/asset-manifest`)
**Updated:** 2026-07-24

Formalized the precursor `generate_manifest.py` into the `nctool` package and
added the missing `validate` mode + a manifest-driven harness. Stdlib only; the
"never read sample pixels" invariant is preserved (metadata via `nc inspect`,
bytes only streamed to hash).

- **Package seed** `scripts/analysis/nctool/` (minimal — full skeleton lands in
  `conversion-metrics`):
  - `manifest.py` — one implementation of `generate` / `validate` / `roles` plus
    shared directory walkers (`walk_rolls`/`walk_samples`/`walk_converted`, and a
    flat `disk_files` for validate) so generate's structured build and validate's
    on-disk set can never diverge. The generate logic is a faithful port of
    `generate_manifest.py` (same seeds, rename/checksum-identity preservation,
    source_frame retarget, encoding inference, atomic write).
  - `__main__.py` — `python -m nctool manifest {generate,validate,roles}` argparse
    dispatcher (`--asset-root`, defaults `$NC_ASSET_ROOT` → `../nc-assets`).
  - `__init__.py` — dependency-free seed docstring.
- **`generate_manifest.py`** retired to a thin backward-compat shim forwarding its
  historical CLI to `nctool.manifest.cmd_generate` (the skill/docs reference it by
  path; no `PYTHONPATH` needed since it inserts its own dir).
- **`validate`** (new) — reports **drift** (recorded sha256 ≠ file, with a
  byte-size pre-check before hashing hundreds of MB), **missing** (in manifest, off
  disk), **orphans** (on disk, untracked); lists regenerable no-sha outputs as
  `unchecked`. REPORTS only, never deletes. Exit 0 clean / 1 discrepancies / 2
  operational (no/invalid/unsupported-schema manifest). Does **not** need `nc`.
- **Harness** `scripts/real-scan-verify/harness.sh` — replaced the hard-coded
  `ROLLS` array with a `while read` loop (bash 3.2-safe) filling it from
  `nctool manifest roles`. Only rolls with exactly one unexposed + one leader are
  emitted (NLP-source roll skipped), reproducing the original five-roll set. Fails
  loudly (exit 2 + remediation) if the manifest is absent.

**Verified (all on this build, macOS/aarch64):**
- `generate` on the live assets reproduces the existing/live `manifest.json`
  **byte-identical** (full sha256 recompute of ~12 GB, ~10 s): 6 rolls, 5 samples,
  buckets `nc/2026-07-22` (34, regenerable), `nc/V0` (8), `nlp/2026-07-23` (4),
  same coverage_gaps. Shim path reproduces it too.
- `validate` on the clean tree → 0 orphans/missing/drift (exit 0). A synthetic
  tree with a deleted / added / edited file surfaces missing + orphan + drift
  (exit 1).
- `roles` emits the five calibration triples with unexposed/leader **matching the
  hard-coded ROLLS exactly**; `Portra160-2026-07-22` (all real) correctly skipped.
- **Harness parity:** ran `freeze` with the old hard-coded array vs the new
  manifest-driven harness on the same binary → **byte-identical `recipes/`**. The
  new `.json` / `.provenance.json` also match the committed set; the committed
  `.hdr.json` differ **only in JSON key order** (`output` vs `reconstruction`
  position, identical values) — a pre-existing artifact of an older harness jq
  revision, unrelated to this change and reproduced by the old array too. Repo
  `recipes/` left untouched (restored to committed state after the A/B).

**Notes for `conversion-metrics` / `nlp-comparison`:**
- The package is intentionally minimal: `__main__.py` dispatches only the
  `manifest` group. When adding `metrics` / `thumbs`, either extend `__main__.py`
  or introduce the documented `cli.py` and have `__main__` delegate — the shared
  walkers and `iter_meta`/`load_manifest`/`Prev` in `manifest.py` are reusable.
- `python -m nctool` needs `scripts/analysis` on `PYTHONPATH` (the harness and docs
  set it inline); the shim avoids that only because it inserts its own dir.
- Open question for the user: the committed `recipes/*.hdr.json` key order lags the
  current `harness.sh` jq (values identical). Harmless, but a `freeze` re-run will
  reorder those keys — decide whether to refresh the committed recipes.

**Update 2026-07-24 (review-fix round) — hardened `validate` + tests.**
Addressed the `asset-manifest` review findings (all uncommitted, in worktree):
- **Full-tree orphan scan** (`all_disk_images`): `validate`'s orphan check now
  walks the entire asset tree recursively for `.tif`/`.tiff`, so a root-level stray
  or a deeply-nested scan (`samples/icc/sub/x.tif`, `rolls/<roll>/sub/x.tif`) is
  flagged instead of being invisible to the structured generate walkers (left
  unchanged). Non-image companions (`.json`/`.jpg`) and `manifest.json` are excluded
  by the extension filter.
- **Fail on missing checksum for irreplaceable/error entries**: entries carrying an
  `error`/`metadata_source:"none"` (new `ERRORS`) and non-regenerable entries lacking
  `sha256` (new `NO CHECKSUM`) are now PROBLEMS (exit 1). Only entries in an explicitly
  `regenerable: true` bucket may legitimately be `unchecked`.
- **`inspect()` loud on nc-parse-failure**: `nc inspect` exit 0 with unparseable/
  missing-key JSON is now a per-file `error` (`metadata_source:"none"`), not a silent
  downgrade to exiftool placeholders. Non-zero exit (rejection) still falls back.
  Happy path unchanged (byte-identical reproduction preserved).
- **Harness roles exit-code**: `harness.sh` now captures `nctool manifest roles` to a
  temp file and checks `$?` (process substitution discarded it) — a mid-stream `roles`
  crash fails loud (exit 2 + remediation) instead of proceeding on a truncated ROLLS.
- **`cmd_roles` unknown-role guard**: a typo'd role warns loudly and is folded into
  `real` (was silently bucketed into a phantom key via `setdefault`, dropping a frame).
- **nc-absent is loud**: a wholesale nc-absent `generate` now exits 2 with remediation
  by default; the degraded exiftool-only mode is gated behind `--allow-exiftool-fallback`.
- **`write_manifest` durability**: unique `mkstemp` temp + `fsync` before `os.replace`
  (concurrent-run safe; no zero-length file after a crash).
- **New committed test suite** `scripts/analysis/nctool/test_manifest.py` (stdlib
  `unittest`, hermetic synthetic tree, no real assets, `nc` stubbed): 30 tests over
  roles parity, build preservation/rename/encoding/coverage, load_manifest schema,
  `inspect` parse-vs-rejection, and `validate` classification + 0/1/2 exit contract.
  Run: `PYTHONPATH=scripts/analysis python3 -m unittest nctool.test_manifest`.
- **Verified:** `generate` on live assets still reproduces the manifest
  **byte-identical** (sha `7351955…`); `validate` clean → exit 0; 30/30 unittests
  pass; Rust CI green (fmt / clippy -D warnings / build / 411 tests).


## conversion-metrics

**Status:** done (2026-09-03)
**Updated:** 2026-09-10

- Goal: Formalize the ad-hoc image-library analysis from real-scan verification into the reusable Python toolkit that is the toolkit's single documented entry point.
- 2026-08-12: Folded the briefly separate `photographic-result-analysis` follow-up into this
  task rather than creating a false dependency between overlapping work. The trigger was the
  Portra 400 Dmax 1.2-versus-1.9 comparison: provenance, channel means, and clipping counters
  establish that the runs differ, but do not explain color and tone distribution,
  shadow/highlight occupancy, range use, or proximity to the endpoints. Metric definitions and
  the final artifact design remain opening questions for implementation.
- 2026-09-02: Task file rewritten; three decisions taken before implementation.
  (1) `numpy` + `tifffile` in a venv, replacing the stdlib-only assumption the old
  Design section carried — that section also claimed `harness.sh` would be retired,
  which never happened (it is fixture-tested in CI). The Python CI gate will have to
  install the dependencies on both platforms. (2) Every input's color space is
  declared, never guessed; unstated is a loud refusal. Verified motive: the manifest's
  `encoding` field records depth (`f32`) but not transfer, while the NLP files
  themselves carry a *linear* sRGB profile — measuring them against nc's
  transfer-encoded u16 without decoding would have produced a plausible, wrong table.
  (3) Tone metrics live in log2 stops relative to 0.18, where exposure is an offset and
  contrast is a slope. Also confirmed by survey that nothing in `scripts/` reads output
  pixels today: `roll analyze`, `compare run`, and the `render-defaults` scripts all
  derive their numbers from nc's own JSON report, so they cover nc outputs only. The
  film-holder problem is handled by an explicit fractional region, with the note that a
  5% inset does not clear a real holder and that excluding dark pixels as a holder proxy
  would bias the very shadow statistics being measured.
- 2026-09-02: Tone slice implemented — `nctool metrics image`, the toolkit's first
  reader of output pixels. `scripts/analysis/nctool/metrics.py` declares the colour
  space (never infers it), decodes to linear, adapts to D65, and reports endpoint
  occupancy on the stored samples plus tone statistics in log2 stops relative to
  0.18: key (geometric mean), an eleven-point percentile vector, contrast spreads,
  toe/shoulder spans, and band occupancy. Regions are fractional (`--inset`,
  `--region`). `numpy`/`tifffile` arrive via `scripts/analysis/requirements.txt`;
  the import is lazy so every stdlib command still runs without them, CI installs
  them, and `NCTOOL_REQUIRE_DEPS=1` turns a forgotten install into a failure rather
  than 29 skips under a green `ok` (the guard was checked by running it against an
  interpreter without the packages).
  Colorimetry is not restated: primaries, whites and Bradford are transcribed from
  `definitions.rs`, and tests re-read that file plus the generated
  `derived-artifacts.txt`. Python's independent derivation reproduces the Rust
  audit's binary64 `SRGB_LUMA` and `DISPLAY_P3_LUMA` exactly; a test also pins that
  the BT.2020 derivation from primaries deliberately stays ~2e-6 from the tabulated
  vector, since a linear-light luminance weighting is not the non-constant-luminance
  luma.
  **Verified against an independent source of truth.** On a legacy-preset render of
  Ektar 971 nc reported `clipped_high` = 0.103282844 of samples; the tool measured
  0.103283 at the top code — agreement to the artifact's own rounding. Measuring the
  same file declared `linear-srgb` instead of `srgb` moved the key by 2.22 stops with
  no error raised either way, which is why the declaration is mandatory.
- 2026-09-02: Finding — **on an uncropped frame the film holder *is* the highlight
  distribution, and it is measurable.** The opaque holder blocks all light, so it is
  maximum density in the negative and renders to white in the positive; it therefore
  lands at the top code and dominates every highlight statistic. Measured on Ektar 971
  (`display-p3`, default sigmoid) as the region tightens: samples at the top code
  0.0950 → 0.0134 → 0.0018 → 0.0000 for insets 0 / 0.05 / 0.10 / 0.15, with
  `shoulder_span_stops` recovering 0.000 → 0.186 → 0.385 → 0.417 and the
  `above_diffuse_white` band going 0.0541 → 0. So the render has real highlight
  separation; the full-frame numbers were the holder. The same effect on Portra160
  1102 moves the *median* by 3.1 stops between the full frame and a centre-76% region
  (-2.30 → -5.42), because the white border was holding the whole distribution up.
  Two consequences. **A full-frame measurement of an uncropped scan is not a
  measurement of the picture** — the region parameter is not a convenience, and this
  quantifies what `film-base/ir-holder-detection` would automate. And a first pass at
  this entry read the same pile-up as evidence that `loss.clipped_high` cannot see
  highlight compression on the bounded display shoulder; that was wrong, and the inset
  sweep is what falsified it. `clipped_high: 0` was accurate. The `legacy` render on
  that frame does genuinely clip (nc: 10.3 % of samples), and unlike the sigmoid its
  top-code population survives the holder's removal (2.85 % at inset 0.15).
- 2026-09-02: Adobe RGB support, and where a colour space's definition lives. User
  asked for it (Lightroom exports reach us in that space). Defined as
  `definitions::ADOBE_RGB` in the **Rust**, not in the Python, even though nc renders
  to no such space: primaries living only in `metrics.py` would be a second
  colorimetry source of truth by construction, which is the arrangement CLAUDE.md's
  rule exists to prevent. The metrics tests re-read `definitions.rs`, so a one-sided
  edit now fails on both sides. No `allow(dead_code)` was needed — `cargo build` and
  `clippy --all-targets -D warnings` are clean with the constant unused by the
  runtime — and `derived-artifacts.txt` is untouched, since no pinned artifact derives
  from it. Its red and blue primaries are Rec.709's exactly and only green moves, so
  both suites assert that relationship rather than just the values; that is the pair
  most likely to be transcribed wrongly. Transfer is the pure 563/256 power law with
  no linear segment, verified end to end: a file encoding exactly 18% linear grey
  measures 0.000 stops.
  Two process notes. A first version of the luma test asserted the published
  `[0.2974, 0.6273, 0.0753]` from memory and failed at 5.5e-5; published RGB→XYZ
  tables round D65 to five decimals where `definitions::D65` rounds to four, and the
  remembered digits could not be checked against any source in the repo. It was
  replaced by the checkable relationship (weight moves off green onto red versus
  Rec.709) plus a deliberately coarse three-decimal bound. And adding the space left
  `--help`'s hand-written space list stale with every gate green — the same defect
  CLAUDE.md records for `OutputPreset`'s help text. The parser now builds that list
  from `metrics.SPACES`, and a test asserts it; the test was confirmed to fail when
  the list is hardcoded again. It has to collapse whitespace first, because argparse
  wraps a long name across lines at its hyphen.
- 2026-09-02: Review round on the tone slice (`/code-review`), 11 findings, all
  fixed. Three were real defects that produced a *plausible wrong answer* rather
  than an error, which is the failure class this module exists to avoid.
  **(a)** A planar-layout TIFF (`PLANARCONFIG=2`) was accepted and measured. The
  guard tested the decoded array's shape, but tifffile hands a planar file back as
  `(samples, height, width)`, which passes `ndim == 3 and shape[2] >= 3` — so a
  30x20 RGB file measured as a 20x3 image with 27 of its 30 rows silently dropped,
  exit 0. Reproduced three ways before fixing; note the finding as first written
  did *not* reproduce, because writing an `(H, W, 3)` array with
  `planarconfig="separate"` produces a malformed file rather than a planar one —
  the real reproduction needs the array already in planar order. The check now
  reads the file's own `planarconfig`/`samplesperpixel`/dimensions instead of
  trusting the array's shape.
  **(b)** `parse_region` let NaN through every bound check (NaN compares false
  against all of them) and died later in `int(round(nan * width))`.
  **(c)** `--region ""` bypassed the region/inset mutual exclusion via a truthiness
  test, so a run silently measured the inset while the user had asked for a region.
  Also: a non-TIFF input escaped as a `TiffFileError` traceback instead of exit 2
  (pointing this at a JPEG is the likeliest mistake there is); `bands` did not
  actually partition the frame when any sample was non-finite, and the test
  asserting that it did used `rng.random`, which never produces a NaN; two fields
  named `non_finite_fraction` used different denominators (samples vs pixels) and
  are now named for their base; CI's `pip install --user` would hit PEP 668 on both
  runner images and now builds the same venv the README documents.
  Memory measured rather than guessed while fixing the decode's temporaries: 1.18 GB
  peak at 18.66 MP (~63 B/px), ~4.7 GB extrapolated to 10368x7200. The rewritten
  decode is bit-identical on the real frame — same percentiles, same key, and the
  nc `loss.*` cross-check still agrees to rounding.
- 2026-09-02: Colour slice. `color_stats` reports per-channel balance in stops,
  mean cast and chroma, chroma percentiles, neutral share, six hue sectors, and
  **the cast of each tone band separately** — the crossover detector, and the
  reason a whole-frame cast is not enough: the characteristic negative-conversion
  fault moves shadows one way and highlights the other, which averages to nothing.
  Measured on the Portra160 1102 pair it separates cleanly — nc goes `b* = -0.7`
  (deep shadow) → `-32.2` (mid) where NLP goes `-0.1` → `-3.8`, with nc's blue
  running 0.715 stops hot against green.
  Three design points. CIELAB's reference white is **derived from this module's own
  D65**, not the tabulated `(0.95047, 1, 1.08883)`: the headline number is a cast,
  so an RGB-neutral frame has to read `a* = b* = 0` exactly or every image acquires
  a constant tint. Verified in all five linear spaces, ProPhoto included, which
  only holds because the Bradford adaptation runs first. `display-output-acceptance`
  pins the tabulated white for its own oracle — that one compares absolute
  colorimetry across renditions rather than relative cast within one image — and
  the two choices coexist deliberately; a test pins the difference.
  Colour **streams in row blocks** because it needs only aggregates: ~13 B/px
  instead of the ~40 B/px an unchunked XYZ→Lab→chroma→hue pass would have added.
  Peak went 1.18 → 1.43 GB at 18.66 MP (~77 B/px, ~5.7 GB extrapolated to
  10368x7200 — inside nc's 6 GiB default, but not by much). A test pins that the
  block size cannot change the answer, and tone/endpoint output is byte-identical
  to the pre-colour run.
  One correction while writing the tests: the circular-mean comment claimed it
  prevents averaging 350 and 10 degrees to 180, which **cannot happen** with
  60-degree bins — no sector spans the wrap, so an arithmetic mean would agree
  today. The operator is still right and stays right if the sectors are ever re-cut
  around hue centres; the comment now says that instead of a failure it prevents.
  The test that went with it only checked the output range, and was replaced by one
  tying `mean_a`/`mean_b`, the derived hue angle, and the sector binning together —
  a units or off-by-one error breaks one of the three and not the others.
  Two defects found by self-review before the slice was reported, both of the
  plausible-wrong-answer kind. `np.histogram(range=...)` **discards** out-of-range
  values rather than clipping them, so a frame more saturated than the chroma
  histogram's 150 ceiling emptied the histogram outright: `mean_chroma` read 208.45
  while `median_chroma` and `p90_chroma` both read 0.12, with nothing to signal it.
  Samples are clipped into the top bin now and `max_chroma` is reported exactly, so
  a saturated percentile is recognizable rather than merely wrong. And colour's band
  fractions divided by its own measured count while tone's divided by the region —
  on a frame with one NaN row colour called a band 1.0 that tone called 0.95, which
  is the same one-name-two-denominators defect the review had already found once in
  `non_finite_fraction`. Every colour fraction is now over the region, with
  `measured_fraction` stating how much of it had a measurable luminance.
- 2026-09-03: Roll rollup and Markdown artifact — the task's last two pieces.
  `metrics roll <roll> <run>` measures every successfully converted frame and writes
  `metrics.json` beside the tag (per-frame records embedded, plus a spread table);
  `metrics table` re-renders it without re-reading pixels. Thirteen tracked axes
  plus two derived crossover terms.
  **The spread, not the mean, is what a roll gets** — frame 3 is a backlit portrait
  and frame 11 a shaded street, so averaging their exposures describes the subjects.
  But a first draft of that claim went further and said the spread *measures the
  calibration*; it does not. One frozen recipe served every frame, so variation
  combines scene content with calibration fit and one roll's numbers cannot separate
  them. Measured on Ektar under `display-p3`: key spread 1.12 stops, `b_over_g`
  spread 0.62 stops across three frames — as easily three different scenes as a
  calibration that does not fit. The wording in the renderer, the artifact note and
  the docstring all say that now, and there is deliberately no outlier rule and no
  verdict; the extreme frames are named so a human can look.
  **The colour space is resolved from the frozen recipe**, which is recorded
  provenance rather than a guess at the pixels, and an under-determined one is
  refused. The table was established by conversion + exiftool, not from the docs,
  which turned up something worth keeping: `display-p3` and `film-master` outputs
  carry the *same* ICC description ("RGB built-in"), so profile metadata cannot tell
  them apart — the preset can. Refused with reasons: the JPEG and AVIF presets, the
  PQ/HLG TIFFs, an `--output-profile` path, and an f32 `legacy` TIFF whose transfer
  was never established. **nc's default is among them**, so the first run of this
  command on a default roll is a refusal with instructions.
  `linear-acescg` was added for `film-master`, which needed the ACES white point;
  the definitions cross-check caught the incomplete transcription immediately (a
  `KeyError` on the white-point map), and a new test now asserts every entry in
  `PRIMARIES` is covered by that check, since the maps are hand-written.
  Two defects the tests found, both mine. `markdown_table` relied on dict insertion
  order, so a record re-read from disk rendered its spread rows **alphabetically**
  while the same record rendered in-run came out in axis order — one artifact, two
  tables. The renderer imposes its own order now. And a test asserting a 2.0-stop
  spread across frames built one stop apart failed at 3.63: the fixture's steps are
  one stop in *stored* values, which under `display-p3`'s sRGB transfer is ~1.8
  stops of linear light. The expectation was wrong, not the code — the test now
  declares a linear space and says why.
  A `metrics-p3-probe` run was converted into the shared assets folder for the
  end-to-end check and removed afterwards (320 MB); its `metrics.json` is kept
  outside the repo. Verified: 188 analysis tests, both interpreters; fmt clean; no
  Rust touched.
- 2026-09-03: JPEG input, and a report pass over the Markdown artifact. User asked
  for it against three points, all of which hold: a gain-map JPEG's ambiguity is
  answerable with a parameter rather than a refusal; an 8-bit-to-8-bit comparison
  still tells the story even if it is not exact; and the NLP outputs that matter
  arrive **as JPEGs**, so "re-render through a TIFF preset" was not an answer for
  the reference side at all.
  `--jpeg-image sdr|hdr` (default `sdr`) selects which rendition of a gain-map
  JPEG to measure. `sdr` reads the base image; Pillow opens an nc gain-map JPEG as
  a single frame — it is registered as JPEG, not MPO — so the appended gain map is
  never touched, which is right for `sdr` and is exactly why the gain map has to be
  detected separately (MPF segment, else a second SOI; `FFD8FF` can only be a real
  marker because entropy-coded data byte-stuffs `FF`). `hdr` is refused with two
  tiers of diagnosis: "this file has no gain map" before "reconstruction is not
  implemented". It is not implemented deliberately — applying the ISO 21496-1 /
  Ultra HDR metadata slightly wrong yields plausible wrong numbers, and
  `hdr-linear-tiff` is the display-linear HDR signal with no container in the way.
  The record now carries `image.container`, `bits_per_sample`, and for JPEGs
  `decoder` (Pillow/libjpeg build) plus `gain_map_present` and `jpeg_image` —
  provenance a lossy read needs, since a JPEG's pixels are whatever its decoder
  says they are. **Verified against the TIFF path**, which already has nc's `loss.*`
  as its oracle: the same content as 16-bit TIFF, 8-bit TIFF and JPEG at q100/q85
  gave identical keys (0.256) and identical p95, diverging only at p0.1
  (-4.117 / -4.119 / -4.132) and `toe_span` — and most of that is the bit depth,
  not the codec, which is the documented caveat reproducing itself.
  **This falsified four of my own tests and three doc claims**, none of which the
  gates would have caught on their own: `gain-map-hdr` and `ultra-hdr-v1` moved
  from refused to `display-p3`, so the preset table, `nlp-comparison`'s "the metric
  reader will not open it", and the README's "nc's default preset is among the
  refused" were all wrong. Found by re-running the suite and then grepping for the
  negation of the claim, per CLAUDE.md.
  Markdown report pass, four fixes: the spread table printed six decimals where
  the per-frame table printed two; `at_top_code` rendered as `1e-06`; fractions
  read as `0.284` rather than `28.38%`; and the build identity was absent from the
  report although it sits in the JSON. Formatting is now per-axis (`AXIS_FORMAT`:
  unit + precision), fixed-point always, with `<0.01` for a value that rounds to
  zero without being zero — "nothing clipped" and "one pixel in a million clipped"
  are different findings. The header names the build and marks `git_dirty` as
  **uncommitted changes**, since a committed report that cannot say which build
  made the pixels is hard to trust later. Percentages live only in the report; the
  JSON keeps fractions, because a field that sometimes means 22 and sometimes 0.22
  is a standing 100x error.
- 2026-09-03 (close-out): Landed as `nctool metrics {image,roll,table}`. A `/ship`
  review round after the close-out above found seven real defects, four of them the
  plausible-wrong-number kind this module exists to avoid, all fixed with regression
  tests. Worth carrying forward:
  **(a)** The three channels' geometric means rested on *different pixel supports* —
  each over the pixels where that channel is positive. Blue crushed to black over
  half a frame reported `b_over_g = 0.0`, a perfectly neutral balance for an image
  with `mean_b = +26.7`. The ratios are now withheld when the supports disagree and
  `color.balance_support` publishes them — the same "counted, not folded" treatment
  `tone_stats` already gave non-positive luminance.
  **(b)** `crossover_a`/`crossover_b` difference the **shadow and mid** bands, while
  four separate places of prose said shadow-to-highlight. Kept the bands (the
  `highlight` band is 0.47 stops wide and often empty, so an axis built on it would
  vanish exactly where a render is darkest) and fixed the prose to say so.
  **(c)** Gain-map detection counted `FFD8FF` across the file, justified by byte
  stuffing — which applies only to entropy-coded scan data, not marker payloads. An
  EXIF APP1 carries a whole embedded thumbnail JPEG, so **every camera and Lightroom
  export** reported a gain map it does not have, and `--jpeg-image hdr` then gave the
  wrong tier of diagnosis. Replaced with a marker walk.
  **(d)** `np.where(finite, encoded, 0.0)` made a NaN sample count as sitting *at
  black*. Comparing `encoded` directly is what the substitution was reaching for —
  numpy comparisons against NaN are already False.
  **(e)** `--output-profile prophoto` was decoded with the ROMM piecewise toe, but
  `color::build_profile` writes a **pure 1.8** power law and says so. Below encoded
  0.03125 they diverge as `16*v^0.8` — 1.3 stops at v=0.01 — exactly what `p0.1`,
  `toe_span` and `deep_shadow` are made of. Two ProPhoto spaces now exist:
  `prophoto` (ISO 22028-2, for third-party exports) and `prophoto-gamma1.8` (nc's).
  **(f)** `metrics roll --space <typo>` decoded the whole roll before failing, then
  reported "no frame could be measured" and returned *before* writing the `skipped`
  list holding the reason. Validated up front now.
  **(g)** `Pillow` was a hard dependency that `HAVE_DEPS` did not import, so
  `NCTOOL_REQUIRE_DEPS=1` — added precisely to stop silent skips — passed without it
  while the JPEG tests errored rather than skipped.
  Verified: 207 analysis tests on both interpreters, fmt, clippy `-D warnings`,
  build, Rust suite (702 + 174). Codex review was unavailable (workspace spend cap),
  so `ship:diff-reviewer` was the sole reviewer.

- 2026-09-10: **Re-cut the tone bands in CIELAB lightness, and added the per-channel
  histogram.** `schema_version` 1 -> 2.
  The old cut was even in stops — `-inf / -4 / -2 / +2 / diffuse white`, after Zones
  III and VII — and even steps of exposure are uneven steps of anything a viewer
  sees: `shadow` 2.00 stops wide, `mid` 4.00, `highlight` 0.47, an 8.5:1
  discontinuity. Measured through the shipped code path on 33 real renders (frames
  G1/G2/G3/E1/E2/P4 through all five `nc convert --preset` bundles, plus the three
  Negative Lab Pro references for the Gold200 roll, `display-p3` / `srgb`, 5% inset):
  `mid` held a **median 82.6% of the frame and 95.0% at worst**, `highlight` read
  under 0.1% on 15 of 33, and `above_diffuse_white` on all 33. Worse than
  uninformative: `cast_by_tone_band` still emitted a `highlight` entry off whatever
  pixels happened to be there, and nothing in the entry said how many. On
  `G2-chr-aim` that band's cast — `a* = -19.5`, the largest colour excursion in the
  record — was the colour of **one pixel** out of 15.1 million, printed beside a
  `mid` cast resting on 91.7% of the frame.
  **Seven cuts were scored on the same 33 renders before choosing**, every one
  digitized off the same `stops` array `tone_stats` uses, so the comparison is
  exact rather than re-binned. Largest band as a share of the frame (median over
  the 33, then the worst single render), and how many of the `33 x bands` band
  shares came out under 0.1%:

  | cut | bands | median | worst | <0.1% |
  |---|---|---|---|---|
  | A old: stops -4 / -2 / +2 / white | 5 | 82.6% | 95.0% | 68/165 |
  | B Adobe: quartiles of the encoded axis | 5 | 64.5% | 79.3% | 43/165 |
  | C Zone: 1-stop bins | 10 | 37.4% | 53.9% | 96/330 |
  | D Zone framing, `mid` subdivided | 6 | 58.7% | 72.4% | 68/198 |
  | E L\* 20 / 40 / 60 / 80 | 6 | 53.2% | 66.2% | 45/198 |
  | **F L\* 15 / 30 / 45 / 60 / 75 (chosen)** | 7 | **46.3%** | **56.2%** | 44/231 |
  | G L\* every 12.5 | 9 | 39.7% | 52.7% | 72/297 |

  (Every cut's `above_diffuse_white` band accounts for 33 of its own `<0.1%`
  count: all 33 renders are SDR. Net of it, A is 35/132 and F is 11/198.)
  **B**, Adobe's parametric-curve splits converted to stops (-1.82 / +0.25 /
  +1.54, widths 2.07 / 1.29 / 0.94), is the reference that named the *shape* of
  the answer — narrowing smoothly, because equal steps in an encoded domain
  compress in stops — but it is not a spec to copy: Adobe's regions are
  overlapping weighting regions for editing, not disjoint measurement bins, and
  at four regions one still takes a median 64.5%. **C**, the literal Zone system,
  is eleven equal 1-stop bins; on a display-referred render that is the wrong
  shape at both ends — 96 of its 330 entries under 0.1%, three whole bands whose
  *median* is under 0.1%. **D**, the cheapest diff, keeps the Zone framing and
  splits `mid`; at 58.7% median it shows the framing was the problem, not the
  band count.
  **Chosen: equal steps of CIELAB L\***, every 15 to L\* 75, then diffuse white
  (L\* 100), then an overflow band. Seven names: `deep_shadow, shadow, low_mid,
  mid, high_mid, highlight, above_diffuse_white`. Why L\* rather than the encoded
  axis Adobe splits: it is the **same perceptual space the colour stage already
  measures cast in**, so the two stages now share a domain and not merely a list of
  edges — and splitting sRGB's curve would have made nc's own output encoding the
  authority for measuring everyone else's. The two agree closely anyway (an equal
  five-way split of the sRGB axis lands at -2.44 / -0.44 / +0.82 / +1.75 against
  L\* 20/40/60/80's -2.59 / -0.68 / +0.64 / +1.65), which is itself the argument
  that the family is right and the choice within it is not delicate.
  **Measured before -> after, same code path, both cuts:** largest band as a share
  of the frame, median **82.6% -> 46.3%**, worst **95.0% -> 56.2%**; `highlight`
  median **0.85% -> 3.84%** and frames under 0.1% **15/33 -> 6/33**; `deep_shadow`
  frames under 0.1% **20/33 -> 5/33**; cast entries resting on under 0.1% of the
  region **18 of 120 -> 12 of 200**. The headline is discrimination: across the
  five presets of frame G2 the old band vector spread **2.1 percentage points**
  (`mid` 91.70 / 93.78 / 91.81 / 93.26 / 92.84 — five presets, one reading), the
  new one spreads **24.0** (`low_mid` 51.34 / 27.32 / 49.71 / 30.02 / 28.97).
  **Why 15 and not 20, and why stop there.** L\* 20/40/60/80 (**E**) gives six
  bands and the same round story, and was rejected on measurement: its largest
  band still takes 66.2% of one real frame against 56.2%. Going finer stops
  paying — L\* every 12.5 (**G**) buys 3.5 points of worst case for two more
  bands, one of which reads under 0.1% on most frames, and takes the sparse-entry
  rate from 5.6% to 14.8% (net of the SDR-empty overflow band).
  **`above_diffuse_white` stays, and is a deliberate exception to "no band empty on
  a normal frame".** It is an overflow bin: an SDR rendition cannot populate it,
  but on `film-master` or `hdr-linear-tiff` it is the only place in the tone stage
  where headroom above display white appears, and `endpoints` cannot stand in for
  it on a float file. Documented as such rather than quietly dropped.
  **The population rule is a caveat, not a filter.** Every `cast_by_tone_band`
  entry now carries `pixels` and `sparse` (under `BAND_SPARSE_FRACTION`, 0.1% of
  the region). Sparse entries are **kept** — a band set that varies frame to frame
  cannot be diffed — but the rollup's `crossover_a`/`crossover_b` are withheld when
  either contributing band is sparse, because a difference of two means is only as
  good as the thinner of them and a roll spread cannot say which frame was thin.
  `highlight` is now a tracked rollup axis; on the old cut an axis built on it said
  nothing about a roll.
  **`tone.histogram`**: the record's first list-valued field. Four series
  (`luminance`, `r`, `g`, `b`), 100 counts each, one per L\* unit from black to
  diffuse white, plus per-series counters for samples above diffuse white and for
  those with no lightness (non-positive, non-finite) — they partition the region,
  and a test pins that. L\* and not the stored code values, which describe the
  file's encoding as much as the picture; L\* and not stops, which give black an
  unbounded tail no chart can draw; and because the bands are cut on the same axis
  **every band edge falls exactly on a bin edge**, so one chart can shade bands
  over bars without interpolating (a test breaks if either the edges leave integer
  L\* or the bins leave L\*). The channel series apply the same L\* curve to one
  channel, which is a level and not a colorimetric lightness — stated in the record
  rather than left to be inferred. It streams in row blocks like `color_stats`:
  measured **+0.55 s** (2.42 -> 2.97 s) at 18.7 MP and **no measurable memory**
  (1.40 GB both ways); a record grows to ~6.8 KB.
  **The record states its own band cut** (`record.bands`: domain, names, L\* edges,
  stops edges, sparse threshold), because the edges have now moved once and would
  read plausibly against the wrong definition if they move again. `metrics table`
  **refuses** a record whose `schema_version` is not the current one: every column
  label still fits a schema-1 record, so rendering it would silently compare two
  definitions of shadow. Checked the other consumers — `nctool roll` and `nctool
  compare` carry their own schema constants and never read a metrics record, and
  `scripts/real-scan-verify/` does not use `metrics` at all.
  Verified: 226 analysis tests (207 before) on the venv interpreter, and the four
  new invariants were each confirmed to fail when deliberately broken — the
  sparse flag in both directions, the schema refusal, and bin/band alignment
  broken two ways (a fractional L\* edge, and a histogram binned in something
  other than L\*). The stdlib interpreter still skips cleanly (87 skips, 226 run).
  No Rust was touched, and the Rust gates were run anyway and are green: fmt,
  clippy `-D warnings`, build, 755 + 191 tests.

- 2026-09-10 (follow-up): **Histogram range extended past diffuse white; survey of
  how other tools bin tonal regions.** Supersedes the histogram range described in
  the entry above.
  **The range now runs L\* 0..200 in 200 bins**, not 0..100 in 100. Two reasons,
  both of which the first version got wrong. A float or HDR rendition genuinely
  carries samples above display white and a scalar overflow counter **cannot be
  drawn** — L\* 200 is 6.46x diffuse white (+5.17 stops), covering nc's own
  1000/203 HDR ceiling (L\* 181.4) with margin. And putting white at the *edge* of
  the axis hid the commoner SDR question: how far short of diffuse white the
  highlights stop. Measured on the five preset renders of G2, the last non-empty
  luminance bin sits at L\* **88 / 92 / 88 / 92 / 98** — 12, 8, 12, 8 and 2 L\*
  short of white, with `sig-knees` the only one that nearly reaches it. Diffuse
  white is now the bin-100 boundary, exactly halfway along, and the per-series
  overflow counter is renamed `above_range` (it no longer means "above diffuse
  white", which the bins themselves now resolve). Cost is unchanged: +0.60 s at
  18.7 MP, no measurable memory, a record 6.8 -> 8.2 KB with ~107 empty bins on an
  SDR frame — the price of one axis that serves both SDR and HDR.
  The record now also names `mid_grey_bin` (49) and `diffuse_white_bin` (100), so
  a chart does not re-derive the L\* formula to place its two reference lines.
  **A test pins that the `luminance` series and `tone.percentiles_stops` describe
  the same quantity**: both take the declared space's own luma weighting, and the
  test brackets every one of the eleven percentiles into the bin its cumulative
  count lands in. Worth recording *how* it was falsified, because the obvious break
  does not work — monkeypatching `luminance_weights` moves the tone stage and the
  histogram *together*, so consistency survives and the test passes. Breaking only
  the histogram's weighting is the real check: an equal-weight luma fails it, and
  so does a Rec.709 luma on a Display P3 file and a 1% error in the green
  coefficient — but only after the fixture was made strongly channel-separated and
  the assertion widened from the median to all eleven percentiles. At the first
  attempt (median only, mild cast) the Rec.709 swap passed.
  Luminance is **emitted, not left to be derived at draw time**: luma is a weighted
  sum of linear channel values and is not recoverable from three independent
  per-channel histograms.
  **Survey of how other tools bin tonal regions, since nc's cut should not rest on
  one vendor's reverse-engineered defaults.** Adobe's parametric-curve splits
  default to 25/50/75 of the **encoded** axis; the conversion to stops re mid grey
  was recomputed here rather than taken on trust and it checks out — -1.823 /
  +0.250 / +1.538, with diffuse white at +2.474. The wider finding is the useful
  one: **no surveyed tool defines disjoint bins for *measurement*.** Every tonal
  region that could be checked is an *editing* construct, and they are overlapping
  weighting regions, not bins — Adobe's parametric curve by its own description,
  darktable's `color balance rgb` by alpha masks with a luminance fulcrum set where
  all three masks reach 50% opacity, RawTherapee's shadows/highlights by a "tonal
  width" measured in from each end.
  The one disjoint binning found is **darktable's tone equalizer: nine zones, 1 EV
  apart, spanning -8 to 0 EV** ("this tab splits the brightness of the guided mask
  into nine zones (from -8 to 0 EV)"; the manual does not state the anchor
  unambiguously and it was not pinned). That is a shipping, principled,
  stops-even, Zone-like cut — i.e. candidate **C**, which this task rejected. The
  reason the same cut suits darktable and not this record is **what an empty bin
  costs**: in an editing tool an empty zone is a slider that does nothing, which is
  harmless; in a measurement record it is a reported number with nothing behind it.
  Measured, C had the best largest-band share of all seven candidates (37.4%
  median) and the worst sparsity — 96 of 330 band shares under 0.1%, three whole
  bands whose *median* is under 0.1%. So the survey does not overturn the choice,
  but it does mean the stops-even family has a real advocate and the rejection
  rests on the sparsity measurement, not on principle.
  On the histogram's domain the survey is supportive rather than mixed:
  **RawTherapee draws its L curve histogram in CIELAB L\*** ("the histogram on the
  L curve reflects lightness after the Lab adjustments"), which is the same domain
  chosen here. Sources: Adobe Camera Raw / Lightroom tone-control docs, darktable's
  tone-equalizer and color-balance-rgb manual pages, RawPedia's Lab Adjustments and
  Shadows/Highlights pages.
  Verified: 229 analysis tests (226 before), venv interpreter; the new range tests
  were each confirmed to fail when broken (range cut back to diffuse white, and the
  three luma-weighting breaks above). No Rust touched.

## drive-asset-migration

**Status:** not started
**Updated:** —

- Goal: Make working from the Google Drive-hosted asset folder robust across machines, now that the assets — inputs *and* conversion outputs — physically live there (moved and reorganized 2026-07-24, with a self-relative `manifest.json` at the root). The move and reorg are done; this task covers the remaining robustness/tooling and the repo path-convention decision.


## nlp-comparison

**Status:** not started
**Updated:** 2026-09-11

- Goal: Ingest Negative Lab Pro (NLP) conversion outputs (the user adds them to `nc-assets`) and compare them against nc's outputs: global per-image metrics side by side, plus side-by-side downscaled thumbnails.
- 2026-09-02: Task rewritten and widened from "NLP vs nc" to reference comparison,
  after the user pointed out that NLP is not ground truth — they edit its results and
  can contribute those edits as assets. References therefore carry a role: `reference`
  (another tool's output as it came) versus `target` (an image edited to the wanted
  result). That yields three deltas per axis, and makes `|nc − target| < |NLP − target|`
  the acceptance question; the NLP→target spread also supplies the scale for what counts
  as a meaningful difference, instead of a picked tolerance. Re-verified the asset facts
  the no-registration design rests on: `nlp/2026-07-23` is 4406×2930 against a 5184×3600
  source and its **aspect ratio differs** (1.504 vs 1.44), so the crop cannot be undone
  arithmetically — but `nlp/2026-08-04` is full-frame 5184×3600, so an opt-in pixel-wise
  section gated on exact dimension equality will genuinely engage on some sets.
  `converted/SmartConvert/TIFF` is present but carries neither a `source_frame` nor an
  ICC profile, so it is unpaired until both are declared by hand. Noted that nc's default
  gain-map JPEG is unreadable by the planned metric reader, so comparison runs go through
  a TIFF preset.
- 2026-09-10: **First measured nc-versus-NLP numbers, recorded as a starting point
  rather than acted on.** They fell out of the `analysis/conversion-metrics` band
  re-cut, which needed a non-nc producer to score candidate cuts against. Nothing
  here changed a render: the preset brightness target was approved by the user on
  2026-09-09 (see `docs/progress/algo.md`) and re-calibrating it is not this
  evidence's call.
  Three Gold200 frames have an NLP reference for the same source
  (`converted/nlp/2026-07-24/` 1137 / 1144 / 1151 = G1 / G2 / G3). Measured with
  `nctool metrics image --inset 0.05`, nc through `--preset chr-generic` and
  `sig-flat` at `--output-preset gain-map-hdr` (`display-p3`), NLP read as `srgb`.
  **`metrics` was not built to compare across producers at this precision**: the
  NLP files are cropped to 4897x3265 against a 5184x3600 source, so a 5% inset of
  each is not the same picture content, and there is no registration.
  **The colour space of the NLP file is unresolved, and the choice moves every
  number.** Its ICC description is `sRGB IEC61966-2.1 (Linear RGB Profile)`, which
  is self-contradictory, and `exiftool` reports `ColorSpace: Uncalibrated`. Samples
  are 32-bit float and **not** bounded to [0, 1]: full-frame min -0.022096, max
  1.284324, with 0.008% of samples below 0 and 0.096% above 1 — an unclamped
  export, which is consistent with either reading. The distribution argues for the
  gamma reading without proving it: full-frame median 0.5646 decodes to +0.63 stops
  over mid grey if sRGB-encoded and to +1.65 if linear, and a normal photograph's
  median does not sit 1.65 stops over mid grey. **That is plausibility, not evidence
  from the file**, so both readings are reported below and the conservative (gamma)
  one is used for the comparison. Resolving it needs either the NLP/Lightroom export
  setting from the user, or a known-value target pushed through the same NLP path —
  do that before any acceptance number is derived from these files.
  Median luminance, in stops over mid grey — nc `chr-generic` / `sig-flat`, then NLP
  read both ways:

  | frame | nc chr-generic | nc sig-flat | NLP (gamma) | NLP (linear) |
  |---|---|---|---|---|
  | G1 | -0.71 | -0.78 | -0.19 | +1.22 |
  | G2 | -0.05 | -0.10 | +0.77 | +1.70 |
  | G3 | -0.99 | -1.11 | -3.86 | -0.71 |

  **The "nc is darker" reading is not established, and which way it goes on one
  frame depends on the unresolved colour space.** Under the conservative gamma
  reading it holds on two frames and **reverses on the third**: NLP's median sits
  0.52-0.59 stops above nc's on G1 and 0.82-0.87 above on G2, but **2.75-2.87
  stops below** on G3, where 56.2% of the frame lands in `deep_shadow` against nc's
  0.20-0.29%. Under the linear reading NLP is brighter on all three (+1.93-2.00,
  +1.75-1.80, +0.28-0.40). So the reversal is a property of the *gamma* reading,
  not a fact about the two tools, and the direction cannot be stated at all until
  the colour space is resolved. Three frames could not settle it in any case, and
  G3 is the frame `sigmoid-baseline`'s fixtures already flag as exceeding SDR range
  (sky best at +0 EV, trees at +2) — exactly where a per-frame auto-adjustment and
  a frozen recipe should disagree most.
  **The difference that is consistent across all three frames is contrast, not
  brightness.** nc's `contrast.p95_minus_p5` is 3.55-4.51 stops on every frame and
  both presets; NLP's is **6.98 (G2) / 10.92 (G3) / 11.52 (G1)** under the gamma
  reading and **3.83 / 7.35 / 8.14** under the linear one. Under gamma NLP is wider
  on all three, by +3.28 to +7.60 stops. Under linear it is wider on G1 (+4.09 to
  +4.22) and G3 (+2.84 to +3.03) but **essentially tied on G2** (+0.13 to +0.28),
  so the gap survives both readings on two frames and collapses on one — still a
  stronger signal than the median, which survives neither cleanly. What holds
  unconditionally is the *stability*: nc's figure moves 0.96 stops across the three
  frames where NLP's moves 4.54 (gamma) or 4.31 (linear), the signature of one
  frozen recipe against a per-frame adjustment. That, not the median, looks like
  the thing worth investigating first.
  **Highlight occupancy on G2 is the one comparison that survives everything so
  far**: NLP 19.4% of the frame in `highlight` against nc's 1.4-1.8% under the
  gamma reading, 68.0% under the linear one — same direction, larger under the
  reading that is not being used. It does not generalize, though: on G3 nc holds
  **more** (15.9% against NLP's 14.0%).
  All of these are single-frame measurements of differently cropped images with an
  unresolved reference colour space. They are a starting point for this task, not a
  finding about either tool.
- 2026-09-11: **The colour space is resolved: the NLP files are linear, sRGB/709 primaries,
  32-bit float — so the "gamma reading" used above is the wrong one and every number
  derived from it should be read as superseded.** Resolved from the files themselves, not
  from the user: the embedded ICC profile (520 bytes, all 11 files in
  `converted/nlp/*/`) carries `rTRC`/`gTRC`/`bTRC` of type `curv` with `count=1,
  gamma=1.00000`, and `rXYZ = (0.436035, 0.222488, 0.013916)`, which is sRGB/Rec.709
  adapted to D50 (AdobeRGB's red colorant would be ~0.6098). **The lesson is where the
  authority lies:** `exiftool`'s `ProfileDescription` says
  `sRGB IEC61966-2.1 (Linear RGB Profile)` — self-contradictory, which is what made this
  look unresolvable — while the TRC and colorant tags are the actual definition and are
  unambiguous. Parse the profile, never the description. (The user recalled the export as
  16-bit AdobeRGB; the files disagree, so that recollection is of a different export.)
  Consequences for the entry above, all of which used the gamma reading:
  **`--space linear-srgb` is correct.** NLP's median is above nc's on **all three** frames
  (+1.22 / +1.70 / −0.71 against nc's −0.71..−1.11), so there is **no G3 reversal** — that
  was a decode artefact, and "nc renders darker than NLP" holds on this roll after all.
  Contrast: NLP 8.14 / 3.83 / 7.35 against nc 3.55–4.51 — wider on G1 and G3, **tied on
  G2**. The G2 gap splits 43% shadow / 57% highlight, so it is not shadow-led. The one
  finding that never depended on the reading stands: nc's `p95 − p5` moves **0.96** stops
  across the three frames where NLP's moves **4.31**.
  Unchanged caveats on those three frames: one roll, and NLP cropped to a different
  aspect ratio with no registration.
- 2026-09-11: **The NLP reference set is two export regimes, and the larger one is far
  better evidence than the frames measured above.** Surveyed every file by parsing its
  embedded profile (43 TIFFs; `**/*.tif` — a `*/*.tif` glob misses `2026-09-09`, which
  nests a subdirectory):

  | files | depth | primaries | TRC | directories |
  |---|---|---|---|---|
  | 11 | 32-bit float | sRGB/709 | linear (gamma 1.0) | `2026-07-23`, `2026-07-24`, `2026-08-04` |
  | 32 | 16-bit | Adobe RGB (1998) | gamma 2.1992 | `2026-09-09/2026-09-09-Ektar` |

  So **a declared space is per directory, never per set** — measuring the whole reference
  folder with one `--space` would be wrong for one regime or the other. The 16-bit batch is
  also unambiguous, its `desc`, colorant primaries and TRC all agreeing, where the
  float batch's description contradicts itself.
  **And the 32-frame batch is pixel-aligned with its sources** — every sampled pair has
  identical dimensions (e.g. 4945x3350, 4936x3352), sources present under
  `rolls/2026-09-09-Ektar/` and named in the manifest. That is the condition this task's
  design reserved the opt-in pixel-wise section for, so it now genuinely engages: 32 frames
  of one stock, one calibration, no registration problem and no colour-space ambiguity.
  Prefer it over the three Gold200 frames for any number that has to hold up.
  Follow-up is `algo/contrast-latitude-spike`.

## display-output-acceptance (continued)

**Status:** not started
**Updated:** 2026-07-30

- 2026-07-30: Added a quantitative master/display tonal-delta gate. The
  reference-anchored reconstruction sigmoid owns the toe; normalized display
  outputs may differ for declared transfer/reference-white/highlight/gamut
  reasons but fail if they introduce a second shadow-floor lift or broad
  midtone re-grade. Numeric bounds must be established from the frozen real-scan
  baseline before default activation rather than replaced by a visual-only
  judgment.


## comparison-review-tooling

**Status:** done
**Updated:** 2026-09-12

- Goal: promote the ad-hoc review pages built during `algo/reference-anchored-sigmoid` into a
  maintained tool for comparing rendering configurations by eye. Requested explicitly by the
  user rather than continuing to patch the scripts inline.
- Lessons already paid for and worth preserving: **render through the path being measured** (the
  previews originally used the *legacy* path while the metrics measured `pipeline::sdr::render` —
  reviewing one renderer while measuring another); **click, not hover** (a hover popover covers
  the thumbnail you are trying to leave, and inter-thumbnail gaps make it flicker); **one shared
  lightbox**, which is what makes prev/next possible; **single-quote CSS `url()`** inside a
  double-quoted `style` attribute or the attribute terminates and the tile renders black;
  **never publish these pages** (rendered personal photographs — throwaway dir only, never
  `../nc-assets` or the repo); and **`sips` destroys a gain map when downscaling**, so HDR review
  needs full-size files.
- Wanted: one entry point instead of two overlapping script pairs, the configuration matrix as
  data rather than code, HDR review for frames whose range exceeds SDR, and build-vs-build
  comparison so a future default change can be reviewed the same way.

### 2026-09-02 — the viewer half shipped: `tools/review-app/`

- **Scope was deliberately halved** (user decision): a **viewer only**, plus the data format.
  The generator that renders a matrix and emits the JSON is *not* built — image and
  `review.json` production stays ad-hoc for now. So the task's "one documented entry point
  renders a described matrix" is **not** met yet, and neither are HDR review nor build-vs-build.
  What *is* settled is the contract those will target.
- **The format is the deliverable as much as the app.** `review.json` (`tools/review-app/SCHEMA.md`)
  declares `configs` and `images`, `snake_case` like every other JSON contract here, with image
  paths resolved against the review file so a set is one movable directory. That answers "matrix
  as data rather than code" from the viewer's side. Config order sets both button order and the
  number-key mapping. An unknown config id in `renditions` is a loud error naming the typo (the
  `deny_unknown_fields` reflex); a *missing* rendition is not — it renders as a visible gap,
  because a comparison silently missing half of itself is the worst of the three outcomes.
- **Toggling in place is the whole point, and it is structural**: every rendition of a frame
  occupies one CSS grid cell, inactive ones hidden but still laid out, so switching config
  cannot move the picture by a pixel. Side-by-side hides exactly the highlight differences this
  was built to see. Verified in the browser — all renditions at row 1 / column 1, identical
  bounding boxes.
- **`fullsize` gained pan controls and a mini-map** (the user's sketch): edge buttons stepping
  80% of a viewport, and a window box showing where the viewport sits in the image.
- Stack is **Vite+ (`vp`) + Solid + StyleX + pnpm**, in `tools/review-app/` with its own CI job
  (`pnpm check` / `test` / `build`). Deliberately *not* joined to the Rust matrix: a Rust change
  cannot break it and vice versa.
- **Four traps, every one of which failed silently** — all recorded in the app's `README.md`:
  StyleX drops CSS shorthands it does not model (`background`, `border`, `gridArea` vanished,
  leaving white text on a white button); `stylex.props()` returns React's `className` *and*
  spreading it is not reactive in Solid; a scroll handler that writes a signal creates a
  measure → render → layout → measure cycle that wedged the renderer so hard Chrome could not
  inject a script; and `requestAnimationFrame` never fires in a hidden tab, which silently
  disabled the pan controls and dropped smooth scrolls. Each cost a debugging round.
- **The old lessons above still stand and are not yet re-implemented here.** In particular
  "render through the path being measured" is now the *generator's* obligation, and the
  `sips`-destroys-a-gain-map constraint still blocks HDR review. Whoever builds the generator
  should read that bullet before starting.

### 2026-09-10 — the viewer became fullstack: set loaded by path, watched for changes

- **The friction was setup, not viewing** (user request). `?data=` made a review set something
  that had to be reachable from the served root, so reviewing one cost either a `dist/` copied
  next to it or a hand-built relative URL. The app now runs **TanStack Start** and the server
  reads the set off disk: `pnpm dev <path to review.json>` (a directory works, meaning the
  `review.json` in it), or `REVIEW_SET`. The path may be anywhere. A bare `pnpm dev` still
  renders the committed synthetic example, which is why that example is in the repo.
- **It is dev-server-only by decision** (user). `vp build` stays in CI as a compile check;
  nothing is served from `.output/` and there is no `pnpm start`.
- **Images are served from an allowlist, not a confined root.** Every rendition registers its
  absolute path while the set is parsed and is addressed afterwards by an opaque id, so a path
  is never taken from a URL and `..` in one means nothing — which is also what lets a set
  legitimately name files outside its own directory, as a root-prefix check could not. The id
  carries the file's mtime, so a re-render is a different URL: that is the whole refresh
  mechanism, and it lets responses be cached `immutable`.
- **`renditions` became a plain record.** Start's serializable check is `T extends Map<any,any>`,
  which `ReadonlyMap` fails. `strict: false` would have silenced it; the record is simply the
  better model, being exactly what crosses the wire and what the JSON already is.
- **Live refresh cost four attempts and the failure mode is the lesson.** Restarting the dev
  server under an open page left it showing the previous render with **no error anywhere** —
  indistinguishable from a re-render that changed nothing, which is the one wrong answer this
  tool must never give. Trusting `EventSource`'s retry, replacing the retry, and reloading on
  reopen all failed. Measured cause: after a restart the `changed` event still arrives but
  `router.invalidate()` issues *no request at all*, the router being stale once the module graph
  is replaced — so no repair to the stream could have worked. Recovery now rests on a plain
  `fetch` poll of `/alive`, whose boot id changes with the server; the stream is the fast path
  only. Cost: a restart reloads the page, losing selected config and scroll. Ordinary set edits
  do not.
- **Two Start behaviours that fail silently**, both in the app's `README.md`: `shellComponent`
  renders **server-side only**, so a stylesheet linked there names an asset the client build
  never emits and a browser-side import from there is tree-shaken away entirely (stylesheets go
  through the root route's `head.links`); and a route's `server.handlers` *is* stripped from the
  client bundle, which is what makes importing `node:fs` in a route file legitimate. StyleX
  needed its dev CSS wired by hand for the same root cause — its unplugin auto-injects only when
  Vite's entry is an HTML file.
- **Deliberately dropped:** `loadReview` / `reviewUrl` / `hasDataParam` and their three test
  blocks, the browser no longer fetching anything. The twelve `parseReview` tests survive through
  an injected resolver; the server modules add eighteen, including one that reads the committed
  example off disk so the file-touching path is covered at all. 42 tests, all four gates green.
- **Review caught the headline feature not working, and the reason is a lesson about
  verification.** The held set was dropped only when `review.json`'s own mtime moved, but a
  rendition's mtime is frozen into the asset map at parse time and *is* its `/img/` URL — so a
  re-rendered frame kept its old URL and the browser served it from the `immutable` cache entry.
  Every manual check had missed it because they all used `pnpm dev <directory>`, and the cache
  key compared the *stated* path against the loaded set's *resolved* one; the directory form
  therefore never hit the cache and accidentally looked correct, while the file form — the one
  every doc tells people to use — went stale. Two bugs hiding each other. Both fixed, both now
  covered by tests that were checked to fail against the old code. Test live refresh with the
  **file** form. A third finding: the `renditions` record inherits `Object.prototype`, so a
  config id named `toString` read as a *present* rendition and rendered a broken image where the
  gap belongs; it is built with a null prototype now, which SSR serialization was verified to
  survive.
- **A second reviewer (Codex, on the PR) found four more, three of them the same shape.** The
  watch targets were derived once, so a `review.json` edit moving a rendition into a new
  directory left it unwatched; the starting baseline was taken from disk rather than from the
  *loaded* set, so a render landing between the page's first read and the watcher's first breath
  was recorded as already-seen; and the boot-id baseline was established one poll interval late,
  so a server replaced inside that window was never recognised as different. Each ends in the
  page silently showing the previous render, which is why all three were fixed rather than
  noted. The fourth: the selected config was held as an **index**, so a live edit that removed or
  reordered configs silently moved the selection — it is held by **id** now, verified by removing
  a config from a live set and watching the selection stay put.
- **Not done, and unchanged by this:** the generator is still the reason this task is open, and
  HDR review and build-vs-build are still untouched. The server could now measure `width`/`height`
  itself and retire those schema fields — it does not, and the schema is unchanged.

### 2026-09-10 — styling moved from StyleX to Panda CSS

- **User request, exploratory** ("I want to try to switch to panda-css"). Pure swap of the
  styling mechanism: no visual change intended, and none observed — every declaration in the
  emitted stylesheet was compared against the pre-change one (106 rules each), and the computed
  values were read back out of the browser and compared to the palette. Measure the *emitted
  CSS*, not the source style objects: a first pass quoted a count of the latter that a second
  pass could not reproduce (165 vs 190 vs 207, depending on how the regex treated nested keys
  and multi-line values), while the emitted rules are exact and reproducible.
- **The dev/build asymmetry is gone.** Panda runs as a PostCSS plugin, and Vite pipes every
  stylesheet through PostCSS in dev and in a build alike — so `src/index.css` is the whole
  stylesheet in both modes. That retires `StyleXDevRuntime.tsx` (the hand-wired dev runtime
  the entry above describes), the dev-only `/virtual:stylex.css` link, the `virtual:stylex:runtime`
  module declaration, and the plugin's exclusion from the Vite config's test mode.
- **The `@layer` line in `src/index.css` is the injection point, and losing it fails silently.**
  Panda's PostCSS plugin only treats a file as its output target when an `@layer` at-rule names
  **all five** layers (`isValidLayerParams`: `names.size >= 5 && every(...)`); with no such
  rule it returns early and emits nothing — no tokens, no utilities, no `globalCss`. Because
  that file now holds no literal CSS of its own, the page then renders completely unstyled.
  Measured: trimming the list to the three layers the app actually uses builds at **exit 0,
  no error, no warning**, writing a 0.03 kB stylesheet in place of 6.9 kB. Found in review,
  where the comment above the line had described it as a stylistic ordering choice.
- **`cls.ts` is gone too.** Panda's `css()` returns a plain class string and accepts
  `false`/`undefined` arguments, so `class={css(a, cond && b)}` needs no adapter — the two
  traps `cls()` existed to close (React's `className` spelling, and a spread evaluated once)
  are both absent by construction. Styles are `css.raw()` objects merged by `css()` at the call
  site, so an override *replaces* the base's declaration rather than competing with it in the
  cascade. That is not an improvement: `stylex.props()` merged by property and emitted only the
  winning class too. The two are equivalent here, and a future reader weighing a move back
  should not be told otherwise.
- **The palette moved into `panda.config.ts`** as semantic tokens: `base` is the dark value,
  `_osLight` the light one. Emitted shape is the same as the hand-written custom properties
  (dark on `:root`, light under `prefers-color-scheme: light`), but `color: "fg.dim"` is now
  type-checked and a typo fails `vp check`.
- **The one real trap, measured: a bare number is a *token lookup*, not pixels.** `gap: 16`
  compiles to `var(--spacing-16)` — `4rem`, four times too big — because the default preset has
  a spacing token named `16`; `padding: 18` compiles to `18px` because it has no token named
  `18`. Same syntax, opposite meanings, decided by the preset. Every length in the app now
  states its unit; bare numbers survive only where the property is genuinely unitless.
- **pnpm 11 blocks esbuild's install script** (Panda bundles its config with esbuild), and an
  undecided script makes `pnpm install` **exit 1** — including the install `vp check` runs for
  itself, so the gate fails before it starts. `allowBuilds: {esbuild: true}` in
  `pnpm-workspace.yaml` is the decision; note the field is `allowBuilds`, not the
  `onlyBuiltDependencies` the older docs name.
- **Accepted cost:** Panda's default preset emits its whole token set — ~300 preset colours and
  every spacing/radius/font-size scale as `:root` custom properties, used or not. 16.6 kB of
  CSS, 5.2 kB gzipped, for a 12-colour app. Irrelevant for a dev-server-only tool, so it is left
  alone; the README records the lever (`presets: ['@pandacss/preset-base']`) and its cost.
- **Verified**: all three gates green (`pnpm check`, `pnpm test` — 49 tests, `pnpm build`); the
  dev server serves the generated rules at `/src/index.css` and updates them in place on edit;
  and in the browser, the config buttons, the active fill, the preview borders, the mini-map,
  the pan controls and the scrim all read the right palette values, with the renditions'
  bounding boxes byte-identical across a config switch (the in-place promise) and a clean
  console.

### 2026-09-10 — `strictTokens` on, and the theme became the design system

- **User request**, with the constraint that the `[value]` escape hatch was not to be used
  without a strong reason. None was needed: `strictTokens` + `strictPropertyValues` are on, all
  77 resulting type errors are fixed, and no escape hatch appears in `src/`.
  `strictPropertyValues` was free — it flagged nothing, every enum-valued property already
  naming a real CSS keyword. All 77 were `strictTokens`.
- **`presets` dropped to `['@pandacss/preset-base']`, and that is the substantive decision.**
  Panda's default is two presets doing unrelated jobs, which is easy to miss: `preset-base` is
  the machinery (357 utilities, 107 conditions including `_osLight`, the patterns) and carries
  **no tokens at all**; `preset-panda` is *only* token ladders — 246 colours and rem-valued
  spacing/size/font scales, 422 tokens in all. With `strictTokens` on, keeping `preset-panda`
  would put 422 valid-but-meaningless entries behind every autocomplete, which is the opposite
  of the point. Side effects: the stylesheet went 16.6 kB → 6.9 kB (5.2 → 2.1 kB gzipped) and
  the emitted token block down to 52 custom properties — exactly what the theme declares.
- **Naming rule, applied deliberately.** A value denoting a *specific thing* gets a role name
  (`sizes.thumbWidth`/`thumbHeight`, `sizes.stageCap`, `sizes.panControl`, `fontSizes.key`),
  which retired literals the app repeated across files — 104x70 in three places, 82vh in two.
  `spacing` gets no role names because it has no such structure (the same 8px is a gap, an
  inset and a padding), so it is named by measurement (`spacing["8px"]`): call sites still read
  like CSS, and the gate still holds. Values kept in **px, not the preset's rem**, on purpose —
  this is a pixel-inspection tool, and its chrome should not rescale with the reader's font
  size while the images do not.
- **A token name must not mean two things across categories.** `panel` was both a colour and a
  size, so `backgroundColor: "panel"` was a surface and `maxWidth: "panel"` a column width. Now
  `panelMeasure`. Found by auditing names across categories, not by any gate — nothing warns.
  The one deliberately shared name is `body` (font, font size, line height), where all three
  genuinely mean "the body's".
- **The two escape-hatch temptations, and what they became.** `width: auto` / `maxWidth: none`
  on `fullsize` → `sizes.natural` / `sizes.unconstrained`: "render at the size the file is" is a
  real decision this app makes, so it earns a name rather than `[auto]`. And
  `fontFamily`/`fontSize: inherit` on the buttons → the `body` font and size tokens outright,
  which is the same value today and states what a control matches instead of inheriting it.
- **Verified as a pure value substitution, which is the right check for this shape of change.**
  Every declaration in the emitted utilities layer was resolved back through its token and
  compared with the pre-strict stylesheet: 106 rules before, 106 after, **104 identical** — the
  only two differences being the deliberate `inherit` → explicit body font/size. Also checked:
  every class the SSR'd page emits resolves to a rule in the stylesheet (75 Panda atoms, all
  matched; the 76th class is TanStack Router's own `$tsr`), and the dev-served CSS is
  declaration-identical to the built one. All three gates green.
- **Re-verified in the browser too.** Every computed value read back matches what was recorded
  before the flags went on — palette, 104x70 thumbnails, 82vh cap, 120px floor, 34px controls,
  6px/4px/8px radii, 11px `<kbd>`, 78ch measure, 600 weight — and the buttons' font now
  resolves to the same `-apple-system` / 14px they used to inherit. Switching config still moves
  the picture by zero pixels. The one token worth a live test rather than a CSS diff was
  `sizes.natural`, which exists to beat the presentational `width=`/`height=` attributes: lying
  to an image's `width` attribute (600 against a 2400px file) leaves it rendered at 2400px, so
  the token does the job `auto` did. Console clean.
- **Surfaced, not fixed:** the app has two reading measures, 78ch under the title and 80ch in
  the standalone panels, which looks like drift rather than intent. Both are kept as separate
  tokens because unifying them would move the layout; someone should decide which.
- **Review found no code defect and five prose defects, which is the expected shape here.**
  Both the `@layer` trap above and the corrections in this entry came from it: a
  "theme is exhaustive" claim the same file contradicted 30 lines later (and CLAUDE.md
  repeated without the caveat), five measured figures that did not reproduce, a longhand
  rationale that was true of borders but not of the grid placement it also named, and a
  "strictly better than StyleX" claim that was simply wrong. Codex was unavailable (workspace
  spend cap), so this was a single-reviewer pass. The lesson is the project's own: no gate
  reads prose, so a number quoted from a one-off script survives every green run — re-derive
  it, or say how it was counted.

### 2026-09-12 — the generator half shipped: `nctool review generate`

The remaining half, and what closes the task. `python -m nctool review generate
<matrix.json>` renders every (frame x config) cell a matrix names and writes the
`review.json` for it.

- **The matrix is data.** `scripts/preset-review/presets.matrix.json` replaces the Python
  list `generate.py` carried; the script is gone. A config states its own `args`, and the
  per-roll values they need arrive through placeholders — `{dmin}` from
  `scripts/sigmoid-baseline/fixtures.json` (the declaration the metrics already read, so the
  two cannot drift) and `{film_stock}` from the matrix's own `rolls` block. Nothing is
  derived from a name: the fixtures call a roll `2026-07-24-Gold200` where the registry calls
  the stock `gold-200`. Which configs take a stock is therefore stated by their args, which
  is the property the old script protected with a hand-maintained boolean column.
- **Placeholders are validated when the matrix loads**, not when a frame renders: `--film-stock
  {film_stok}` would otherwise reach `nc` as a literal stock name, 35 renders in.
- **The file suffix comes from the matrix's preset** (mirroring `cli::derived_extension`;
  `nc` refuses a mismatched `-o`, so a stale entry fails loudly), but **the colour space each
  cell is measured in comes from the recipe `nc` reports it resolved**. The first version read
  both off the preset name, which is wrong for `legacy` and `custom`: they accept
  `--output-profile`, which a matrix is free to pass, so a ProPhoto render would have been
  measured as sRGB — every tone and cast number wrong while every one of them still looked
  reasonable, which is the plausible wrong answer `metrics` exists to refuse. `space_for_recipe`
  also declines an `f32` output whose transfer is unverified, and that now costs one cell's
  charts instead of being papered over. A matrix restating a flag the generator owns
  (`--output-preset`, `-o`, `--report`) is refused rather than silently overridden — `nc` takes
  the last occurrence of a `Set` argument, and `--output-preset` decides the suffix too.
  A preset the metrics cannot read at all (`hdr-pq` writes AVIF) still renders a reviewable
  page — without charts, and saying why, once up front.
- **Each rendition gets its metric record written beside it**, which is what the app's charts
  draw. Re-measuring is skipped when a stored record already carries that file's **checksum**
  and the same region — not its mtime, which a re-render always moves. Measuring a 74 MP frame
  is minutes of work.
- **Failure is per cell.** A roll with no stock loses one column, not the frame; a failed
  render is reported and its config simply has no rendition, which the page draws as a gap.
- **Each measured rendition also gets its `width`/`height`**, so the page can reserve the
  box before the image loads — the record is the only source, since `nc`'s report carries no
  image size. A `--no-metrics` set therefore still omits them, which the schema allows.
- **It refuses an output directory inside the repository.** The frames are the user's own
  photographs; the old script relied on the operator remembering, and this is now the
  blessed entry point. Argument checks run **before** environment checks, so `--out .` is
  told about `--out .` rather than about an unbuilt binary — CI caught the original order,
  because it builds only the debug binary.
- **Reuse is keyed to the declared space and the JPEG decoder as well as the checksum**,
  and colliding cell names are refused up front — case-folded, because the default macOS
  volume treats `A-c.jpg` and `a-c.jpg` as one file. `<frame>-<config>` is not injective
  when either id may carry a hyphen, and config ids routinely do. Ids are checked
  filename-safe for the same reason the output directory is: both sources are inputs, and
  one holding `../` writes outside the directory that was just validated.
- Verified end to end on P3, G2 and E1 — one frame from each of the three rolls — across all
  five presets: 15 renditions + 15 records, exit 0. The matrix's `metrics.inset` of 0.18 does
  clear the film holder on all three (0.000% of pixels below L\* 5; a holder in the region
  reads as a hard spike at the bottom of the histogram, which is the check, and the app draws
  that histogram). A second run re-rendered every cell byte-identically and **re-measured
  none** of the five it already had — the checksum reuse path, on real data.
- 55 hermetic tests (`test_review.py`); the analysis suite is 284, up from 229. The matrix
  is read with `deny_unknown_fields` discipline, which is not fussiness: `"arg"` for
  `"args"` loads as *no* arguments, so that cell renders the default conversion under a
  label promising something else — five buttons, five labels, identical pixels, exit 0 —
  and `"insets"` for `"inset"` measures the whole frame, film holder included.
- **Deferred with reasons rather than left open** (both in the task file): HDR review, because
  nothing downscales a gain map — the blocker that motivated it — and build-vs-build, because
  identifying two builds in a page is a provenance problem, not a flag.

## metrics-chart-design

**Status:** done
**Updated:** 2026-09-12

- Goal: settle the chart encodings, the rendering technology and the component split,
  independently of the review app.
- 2026-09-10: Split out of `metrics-visualization` at the user's request — "how to
  visualize the metrics" and "integrate the visuals into the app" are two jobs, and the
  first is the harder one. The 2026-09-03 chart ranking and its reasoning stay recorded
  under `metrics-visualization` below; this task is where they get tested against real
  records rather than reasoned about. Executable now: its only dependency
  (`analysis/conversion-metrics`) is done, whereas `analysis/comparison-review-tooling`
  is still `[~]`.

- 2026-09-10: Design canvas drafted against **real** records — frame G2 through the five
  `--preset` bundles, measured with `nctool metrics`. Three findings the 2026-09-03 reasoning
  did not have:
  - **A colour vertex must carry its band's population.** The `highlight` point is the largest
    excursion in every encoding of `cast_by_tone_band` and rests on **under 1 px in 18.7 M** on
    some presets (56 px for `chr-generic`). At equal weight it manufactures a crossover out of
    rounding. Independent of any re-cut.
  - **The presets are brightness-matched, so the curves fan rather than shift**: 0.02 st apart
    at p50, 0.27 at the toe, 0.34 at the shoulder. That fan is contrast, and no scalar in the
    record locates it — the strongest argument for ranking the percentile curve first.
  - **Colour alone stops separating past three overlaid configs** — the `dataviz` reference dark
    steps pass all-pairs CVD at 2 and 3 series and fail at 5. Compare mode must become small
    multiples beyond three.
- 2026-09-10: **Two modes, separated at the user's request.** Compare (n variants) and inspect
  (one variant) are different designs, not one with a parameter. The rule: a chart takes n
  variants only if it still has a free series dimension — per-channel histograms spend it on
  RGB, the hue polar on angle. They also want opposite things from the config toggle, which
  settles the integration half's open question: compare-mode charts draw every config and the
  toggle *emphasises* one (nothing moves); inspect-mode charts bind to the active config and
  swap in place like the picture. An n-variant chart at n=1 is its own design — the legend
  goes, a difference strip has nothing to compare — not merely fewer lines.
- 2026-09-10: Rebased onto the `bands` measurement change (`schema_version` 2) **by
  re-applying this task's split onto that branch's content, not by resolving a conflict
  line-by-line**. The branch edited `metrics-visualization.md` against its pre-split
  version, so a mechanical rebase would have stranded its additions in the wrong half or
  dropped them: the histogram description and the band/bin alignment belong with the
  encodings, the record-size point belongs with transport. Recorded here because the
  dropped half of such a merge is what nothing references and no gate catches.
  Three facts from it that change the encodings: the histogram is the first drawable
  field and its luminance series is the primary single-frame view; bands and histogram
  are both cut in L\* so every band edge lands on a bin edge and a band overlay needs no
  interpolation; and `sparse` is now a field, so the population weighting these artboards
  argued for is read rather than derived.
- 2026-09-11: **v1 component set locked with the user: three charts.** Luminance histogram,
  per-channel histogram, and cast-over-tone as two axis-coloured curves. The task file
  carries the decision and the reasons for every deferral; what is worth repeating here is
  how the cast chart was arrived at, because three encodings were drawn before one worked.
  The **a\*/b\* path was rejected by the user as unreadable** — "this is a path goes through
  a 2d points, it's too hard for me to read" — which no amount of annotating the plane
  fixed. What replaced it was the user's own design: two lines over the bands, y in CIELAB
  with 0 as neutral, **each line coloured by its own value** (a\* green-to-red, b\*
  blue-to-yellow). That inverts the usual relationship — the colour is there to teach the
  axis, not to carry data — and it made the crossover on `sig-flat` (b\* to -20, a\* to +8
  in the top two bands) legible at a glance where the path never was.
  Three implementation facts that cost a round each: the ramp must be **fixed, not scaled to
  the data**, or a mild cast and a severe one look alike; its ends belong at the **measured
  sRGB gamut limit per direction** (at L\* 65 green clips at 42.8 and blue at 54.3, so the
  a\* ramp caps at ±41 and b\* at ±52 — one number for both under-saturates one of them);
  and a **mark and the line beneath it must share one mapping**, or the marker reads more
  muted than its own stroke.
  The governing constraint fell out of the same work: **once colour carries hue it cannot
  also carry config identity**, so this chart is inspect-mode by construction and compare
  mode needs a different cast encoding. That is the two-mode split earning its keep rather
  than an inconvenience.
- 2026-09-11: **The v1 components are built** — `tools/review-app/src/charts/`, with a
  `/charts` demo route rendering them from a committed synthetic record. Geometry is in
  pure `.ts` with tests (90 in the app, up from 49) because **no `.tsx` in that app can be
  tested at all**: `vp test` collects only `.test.ts` under `src/` and gives it no DOM.
  That boundary is not bookkeeping. Two review rounds found four defects that a green
  type-check and 83 tests had passed, every one of them inside a component: a mid-grey
  reference line whose condition (`midGreyBin + 0.5` against ticks at multiples of ten)
  could never be true, so it never drew; a required `bands` prop nothing read; `bounds()`
  destructured inside a `<For>` over a module constant, which freezes at the first record;
  and the x axis equating bin index with L\*, true only because the record currently uses
  one bin per unit.
  **The ramp was the serious one.** `ramp.ts` claimed as its headline property that colour
  is "fixed, never scaled to the data — scale it and a mild cast on one frame looks like a
  severe one on another", and the code normalised by the plotted range, so a frame whose
  worst band was `b* = +2` painted *identically* to one whose worst was `+20`. It was also
  asymmetric: with bounds `[-10, +18]`, `b* = -5` came out at chroma 26 against `+5` at 14.
  Fixed in the code rather than the prose — a fixed `RAMP_REFERENCE` of 20 CIELAB units,
  widened only when a frame exceeds it, symmetric and clamped. A mild cast now renders
  mild, which is visible on the demo: `a*` sits near neutral and reads near-grey.
  Also corrected: a fixture whose channels were independently shaped and so did not
  partition `pixels` (blue summed to 101.2% of the frame) while the test that should have
  caught it checked only luminance; `sparse` read as `=== true` where every neighbouring
  field threw; and a figure cited without its scope.

### 2026-09-12 — accepted as v1

The user accepted the shipped component set ("I'm fine with the v1 components. They are
enough for the current work"), which closes this task. The three items under *Still open* in
the task file stay open as v2 questions — the cast chart's x axis, a compare-mode cast chart,
and how far `sparse` should demote the curve rather than only its mark — and none of them
blocks the v1 set, which is now drawn under every picture in the review app.

## metrics-visualization

**Status:** done
**Updated:** 2026-09-12

- Goal: plot the `nctool metrics` output inside `tools/review-app`, so numeric review
  sits beside visual review rather than in a separate tool.
- 2026-09-03: Filed after the user reviewed the metrics record field by field and asked
  for a visualization next, naming `bands`, `cast_by_tone_band` and `hue_sectors` as
  candidates. The task file records a different ranking and the reason for it: the
  **percentile curve** comes first because two overlaid decompose a difference into
  exposure (vertical gap), contrast (relative slope) and curve shape (where they
  diverge), which is the question the app exists to answer — `bands` is a coarser view
  of the same data and cannot say *where* the change is. `cast_by_tone_band` is second
  but must be drawn as a **path on the a\*/b\* plane**, not as bars: the shape of the
  path is the crossover. `hue_sectors` ranks last because six sectors is coarse and two
  polar charts compare poorly. `endpoints` was added to the list although the user did
  not name it — a per-channel bar is what made a 22% top-code population visibly
  *blue-only* on a real frame.
- The constraint that ranks them: **every chart must overlay two configs**, because the
  app's premise is toggling configs in place.
- 2026-09-03: Rebased onto the viewer half (`tools/review-app/`, merged to main
  2026-09-02). Read its `SCHEMA.md` before starting: `review.json` is
  `configs x images -> renditions`, which is exactly the shape a metrics record has,
  and the schema already calls `images[].note` "the natural home for measured
  numbers".


### 2026-09-12 — the charts landed under the picture

The wiring half. A review set may now name a measurement per rendition, and the app draws the
three v1 charts below the picture, bound to the active config.

- **Transport: a sibling file, read server-side.** `renditions[config].metrics` names a
  record relative to `review.json`. It is not inlined — ~20 kB of histogram counts per
  rendition, written by a different tool at a different time, and a separate file is what lets
  a re-measurement update the page without rewriting the review document. The server reads and
  parses it, so what crosses the wire is the charted subset: measured at **27 kB of payload
  for five records**, which is 5.4 kB each rather than 20.
- `schema_version` stays **1**. The key is additive and optional, so a set written by the new
  generator still loads in an older build, which a bump would have broken for no gain.
- **It joins the watch targets**, stamped beside the renditions. The model holds the *parsed*
  record, so nothing re-reads it until the held set is dropped — a record that nothing stamps
  produces a filesystem event that diffs to no change, and the charts sit on the previous
  numbers with no error anywhere. That is the same failure renditions had before their mtime
  rode in the URL. Records are deliberately **not** in the asset map: that map is the set of
  files the server may serve, and a record has no business behind an `/img/` URL.
- **Placement settled by the encoding, not by taste.** All three v1 charts spend colour on
  what they encode — channel identity, or the sign of `a*`/`b*` — so none has a series
  dimension left for a second config. They swap with the config exactly as the picture does.
  Below the picture rather than beside it, because the stage is the widest thing on the page.
- **Two failure modes, both local.** A rendition with no record renders its picture and says
  it has no measurement; one whose record will not parse says why, where the charts would be.
  Refusing the set over an unreadable record would take four good comparisons down with it.
- **The panel states what was measured** — "the central 41% of the frame, 7.6 Mpx" — because a
  set insets its measurement to keep the film holder out of the statistics, and a reader told
  nothing would take the histogram for the whole picture. That meant parsing `region` into the
  charted subset, which had not needed it before.
- **The panel says which rendition the numbers describe.** A gain-map JPEG is one file
  carrying two, the page hands the browser the file (which an HDR display decodes as the
  HDR rendition), and `nctool metrics` reads the SDR base — so picture and charts can
  describe different renditions of one file unless the charts say which.
- The `/charts` demo route now renders the same `MetricsPanel` the app mounts, from the
  synthetic fixture, so the two cannot drift — and it keeps the degenerate cases a real record
  rarely carries at once (a sparse band, a band with no pixels, a channel past the top of the
  range).
- App suite 128 tests, up from 102; `pnpm check`, `pnpm test`, `pnpm build` green. Verified
  against the real generated set: three SVGs server-rendered per section, the histogram
  spanning the full plot height, the axis labelled 0 to 110.

## harness-regression-tests

**Status:** done
**Updated:** 2026-08-11

- Goal: give `scripts/real-scan-verify/harness.sh` automated coverage, so a change
  to nc's CLI surface cannot break it silently. See
  [the task file](../tasks/analysis/harness-regression-tests.md).
- Filed 2026-08-09 out of the `output/presets` review round. The default flip to
  `gain-map-hdr` broke the harness in three places with all four CI gates green:
  `stage_freeze`'s `jq` generator still wrote the removed `output.hdr` key; the four
  `convert` stages passed `.tiff` paths and hit exit 2; and `stage_convert` failed
  **without an error at all** — `nc roll` had become container-aware, so it succeeded
  and wrote `_positive.jpg`, the `for g in "$htmp"/*_positive.tiff` rename glob
  matched nothing, the float-HDR outputs stayed stranded in `.hdrtmp`, and the stage
  printed its usual `converted <roll>: N frames x2 modes`.
- The silent one is the reason the task exists. An exit 2 is found the next time
  someone runs the harness; a success line over the wrong container in the wrong
  directory is not.
- Second, narrower lesson recorded in the task: the checked-in recipes were migrated
  by hand while the `jq` generator that *writes* them was not, so re-running
  `stage_freeze` would have silently restored the broken state. Coverage that ties
  the generator to the committed recipes would catch that class directly.
- Deliberately left open: whether a fixture-only harness run is possible at all,
  whether it belongs in CI (no assets, no `exiftool` there), and what language it
  should be in — `scripts/analysis/`'s 91 Python tests already run under no CI gate,
  which is worth resolving together rather than adding a third untested surface.
- 2026-08-11: Started implementation after inspecting the harness and the existing
  stdlib `nctool` tests. The committed TIFF fixtures are sufficient for a hermetic
  `freeze` → `convert` run against the real debug binary: region-based Dmin/Dmax
  estimation succeeds (a low-Dmax warning is harmless), and neither Drive assets
  nor `exiftool` is needed. Plan: make recipe/output staging test-overridable, add
  exact artifact postconditions, reproduce the successful-wrong-container failure
  with a fake `nc`, and put the full analysis unittest suite into CI.
- 2026-08-11: Completed. `harness.sh` now uses fail-fast shell semantics, accepts
  an isolated `REC`, renders u16/f32 into a fresh per-run staging tree, requires
  exactly one TIFF+sidecar pair per frame per mode, and publishes only after the
  complete set validates. Wrong suffixes, extra files, ordinary command failures,
  and determinism differences are hard failures; the expected strict failure is
  asserted explicitly. `nctool.test_harness` covers the real fixture-backed
  `freeze` → `convert` path and a fake u16-success/f32-JPEG-success regression that
  must fail before publication. CI now runs all 94 stdlib analysis tests on Linux
  and macOS. Verified: targeted harness tests, full Python suite, fmt, clippy with
  warnings denied, build, and the Rust suite (793 passed, 5 ignored). A real Drive-backed `freeze`
  regenerated 21 files across seven rolls, all semantically identical to the
  committed recipes/provenance after normalizing JSON key order.
- 2026-08-11: Review hardening. Staged and published artifacts are now checked by
  content (TIFF magic and JSON-object sidecars), directory and directory-symlink
  publication targets are rejected before any move, and final artifacts are
  revalidated before the success line. Saved roll reports normalize
  `frames[].output` to the durable u16 / `_hdr` publication paths instead of
  retaining deleted `.rsv-*` staging paths; each raw report must name every
  expected successful staging path before any image moves. The intentional strict probe now
  accepts only warning-promotion exit 1 with both the IR-ignored and strict
  diagnostics; usage/crash statuses and unrelated warnings fail. Hermetic tests
  cover each regression. Verified: 7 targeted harness tests; all 99 analysis
  tests; fmt; clippy with warnings denied; build; Rust tests (793 passed, 5
  ignored).
- 2026-08-11: Sidecar contract correction. Artifact validation now distinguishes
  generic JSON-object roll reports from conversion sidecars, which must carry the
  binary's real envelope: object-valued `meta` and `params`. The negative harness
  case uses parseable `{}` to prove a wrong envelope is rejected before
  publication; successful fakes emit the minimal valid envelope.


## calibration-frame-capture

**Status:** not started
**Updated:** 2026-09-12

- Goal: shoot, develop, scan and register the ColorChecker bracket rolls three other tasks
  name as a precondition, and take a first neutrality measurement against them.
- 2026-09-12 (filed): three tasks each named these frames in their own words and none owned
  producing them — `io/scanner-density-calibration` (the 3×3 + offset fit),
  `algo/sigmoid-parameter-calibration` (bracketed roll + grey card), and
  `algo/split-default-migration` (its release gate names a known-neutral reference, which is
  evidence rather than a task and so was invisible to the graph). The effect was that the
  graph reported work as executable when the thing blocking it was a roll of film that did
  not exist. The protocol agreed with the user 2026-09-08 moved here from
  `io/scanner-density-calibration`, which now points at it rather than restating it.
- Mostly photographic work. The code half is the manifest role for a bracketed target frame
  (exposure offset + lighting recorded alongside it) and whether one measurement command
  serves all three consumers or each wants its own read.

## review-reference-cells

**Status:** not started
**Updated:** 2026-09-13

- Goal: an outside producer's image (NLP export, SmartConvert, hand-tweaked target) as
  a grid cell beside nc's renders of the same frame, brought to a common SDR sRGB JPEG,
  paired by manifest `source_frame`. Asked for by three tasks; filed 2026-09-13.

## review-build-axis

**Status:** not started
**Updated:** 2026-09-13

- Goal: the same frame and config across two builds as toggling cells, labelled from
  the sidecar's `identity` block. Deferred by `comparison-review-tooling` until a
  default moves; two default moves are now filed.
