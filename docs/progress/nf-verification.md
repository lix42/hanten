# Hanten — nf-verification Progress Log

Execution log for the `nf-verification` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

> **Since `nf-core/default-flip` (2026-09-27, `pipeline_version` 8)** the chain this summary calls the new flow is the only one: read "under `--new-flow`" as the default, and "the current chain" as the removed one (the reference build).

Gates that describe the new chain, and the frozen reference build that lets the old paths retire early.

Created on 2026-09-19 as part of the new-flow migration plan (`docs/nf-migration.md`).

- **The reference build is in place** (`reference-snapshot`, 2026-09-22). The reference
  is the `reserve` branch head, which starts at tag `pre-new-flow` (`0da32d0`), and is
  named by commit. `scripts/reference-snapshot/build.sh` builds and caches it, and its
  README pins the invocation, `--preset sigmoid-knees --output-preset display-p3`. In a
  review matrix, give the reference arm `expect_commit`.
- **The new chain's stage goldens are `pipeline::chain_golden`** (`stage-goldens`,
  2026-09-23). A task that changes a new-flow stage's arithmetic or a decode default
  recaptures that stage's vector, and the threaded ones, in the same change.
  `FilmRgbImage::fixture` is the test-only way to enter the chain with chosen values.
  No decode pixel is provably bit-portable, which constrains `fingerprints`.
- **The drift gate's `render` stops at scene correction's input** (`fingerprints`,
  2026-09-28): the decode plus the ACEScg mapping, over samples at the minimum decode
  window. A decode or mapping default change bumps the version; a rendering stage's
  default *value* moves `recipe`, and its arithmetic is only the goldens'.
- **The decode before the 3×3 is exportable** (`film-rgb-export`, 2026-09-30):
  `convert --export-film-rgb PATH` writes an untagged f32 TIFF of the dye layers, which
  `nctool metrics --space film-rgb` measures per channel. It is the point for per-layer
  (`scale`) measurements; `film-master` mixes the layers. `roll --export-film-rgb` (a
  switch) writes each frame's beside its output (`roll-side-exports`, which also
  removed the IR export).
- **`nctool compare`'s set runs on the new chain** (`benchmark-set`, 2026-10-01): one
  fixtures case per ready destination row (a Rust test enforces it), plus the film
  master, `direct` and the default, on both input formats. A case has a `destination`
  and/or `preset` block and runs only on builds of that interface; the record lists the
  rest as `not_run`. The reference build is compared by running it over the same set
  (its blocks state `--preset sigmoid-knees`); pre-migration records are superseded.
- **A review matrix states `destination` for builds at `pipeline_version` 8 and later,
  and `output_preset` for older ones** (the reference build); a matrix mixing both
  states both, and each build takes the flags its banner's `pipeline_version` says it
  speaks (`scripts/analysis/CLAUDE.md`).

## reference-snapshot

**Status:** done (2026-09-22)
**Updated:** 2026-09-22

- 2026-09-19: created with the new-flow plan. Goal: the frozen reference build.

### 2026-09-22 — shipped

- **The tag already existed.** `pre-new-flow`, annotated and pushed on 2026-09-21, is on
  `0da32d0` (#130) and names the sigmoid-knees reference in its message. The `reserve`
  branch starts at the same commit. **User decision: the reference is `reserve`'s head,
  not the tag**, so fixes cherry-picked there reach the reference. The price is that
  "the reference" can move, so it is always named by **commit**: `build.sh` caches per
  commit (a moved `reserve` lands beside the old build) and says when `reserve` has left
  the tag.
- **Deliverable:** `scripts/reference-snapshot/{build.sh,README.md}`. The script builds
  `origin/reserve` in a temporary worktree (`--locked`, target dir outside the repo) and
  caches it at `~/Library/Caches/hanten-reference/reserve/<commit>/nc` with a
  `BUILD_INFO`. It refuses a binary whose `--version` `commit:` line is not exactly that
  commit's 12-digit prefix, which also rejects `-dirty` and `(dirty unknown)`. The first
  version substring-matched the full 40-digit hash, which `--version` never prints, so
  it refused a correct build.
- **The pinned reference:** `--preset sigmoid-knees --output-preset display-p3`
  (user decision). `display-p3` is the same destination `--new-flow` writes (16-bit P3
  TIFF), so the two differ only in the pipeline. **Frames are not pinned** (user
  decision); each consuming task brings its own. Viewing TIFF in a browser goes through
  the same TIFF→sRGB JPEG step already used for outside references, so nothing was built
  for it.
- **Verified: byte-identical across two builds.** Two fresh `build.sh` runs with separate
  target dirs and cache roots (rustc 1.98.1, aarch64-apple-darwin) produced binaries
  with **the same sha256** (`661316fd3b22…`). The fixture render
  (`hdr-48bit.tif --film-base 1,1,1`) was identical in the TIFF (`e0355a9a2513…`) and in
  the sidecar. The reports differed only in `output` and `elapsed_ms`. On that fixture
  the reference and HEAD `9a61136` also render `sigmoid-knees` identically, as expected
  while the legacy path has not moved.
- **Verified: labelling.** A two-build set (reference + HEAD) labelled its cells
  `0da32d063211` (`git_dirty: false`) and `9a61136bfe6e` from their own reports.
- **Found: a mislabelled arm was *shown*, not *flagged*.** `--build ref=<HEAD binary>`
  rendered at exit 0: the cell sat under the name "reference" with HEAD's commit in its
  `producer` block. The build axis deliberately lets a matrix declare no identity, so
  nothing had anything to check against. **Fix (user decision): an optional
  `expect_commit` on a matrix build**, checked in `record_identity` against the same
  derived identity the label comes from. Every cell is checked, including the first,
  which the drift rule alone lets through. A mismatch goes through the drift abort
  (exit 1, no `review.json`, stale set handled the same way). Prefix match either way
  round, since nc prints 12 digits; a dirty or unknown tree does not match. It is an
  expectation, not a label, so review-build-axis's "derived, never declared" rule
  stands. Probed on the real binaries: the wrong binary is refused, the reference
  passes.
- **Known gap, not fixed here: a `--new-flow` cell cannot go in a matrix.** The
  generator passes `--output-preset` to every cell, and `--new-flow` refuses it (exit 2)
  while it has one destination. `nf-destinations/preset-set` is the natural fix. Until
  then the new-flow side is rendered by hand. This matters to `scale-gamma-loop` and
  `benchmark-set`.
- **Converting with the reference after `nf-retire`.** Once the old code is gone, the
  tree has nothing that documents the old CLI's workflow, so the README gained a
  section: the reference's own `docs/using-nc.md` (`git show origin/reserve:…`) is the
  authoritative guide, plus a checked `inspect → estimate → convert --dump-params →
  roll` sequence for the reference config. On the fixture, a roll frame is
  byte-identical to the single `convert`, and `--params <sidecar>` reproduces it. The
  new chain refuses the dumped recipe (no `recipe_version` 2). `update-usingnc-doc`
  now says to point retired usage at the reference guide rather than keep it alive.

### 2026-09-22 — review fixes

- **`build.sh`:**
  - A failed fetch now really builds the local ref. It used to say so but still
    resolve the stale `origin/<ref>` first.
  - cargo runs from inside the worktree, so a caller's `rust-toolchain.toml` or
    `.cargo/config.toml` can't leak into the reference build. A rebuild still gave
    the same binary (`661316fd…`).
  - A reused target dir prefers the newer of `nc`/`hanten`.
  - A failed install no longer leaves `.staging.*` folders in the cache.
  - The hint for a bad cached binary now names `HANTEN_REFERENCE_REBUILD=1`.
  - The "moved past `pre-new-flow`" note fires only for `reserve`.
  - A relative `HANTEN_REFERENCE_TARGET` is made absolute before the build `cd`s into the
    worktree. Otherwise cargo and the binary lookup resolved it against different
    directories, a bug the `cd` fix itself introduced. Caught by both ship reviewers.
- **`expect_commit` is now also checked in the pre-flight**, off the `--version` banner
  (`commit: <hex>[-dirty| (dirty unknown)]`, the same format on both sides of the
  rename). A wrong binary is refused with exit 2 before anything renders or is
  overwritten. The per-cell check stays as the backstop for a binary that changes
  after the pre-flight. Probed on the real binaries: HEAD behind `ref` exits 2 with no
  output directory created.
- **Stale "the tagged binary" prose swept:** CLAUDE.md (three places), `docs/TASKS.md`
  (two), the `render-review-set` skill, `nctool` docstrings and comments, a `cli.rs`
  comment, and the open task files `nf-retire/legacy-custom`, `nf-verification/benchmark-set`
  and `nf-core/buffer-strategy`. Progress logs and this closed task file keep their
  wording, since they record what was true then.

## fingerprints

**Status:** done (2026-09-28)
**Updated:** 2026-09-28

- 2026-09-19: created with the new-flow plan. Goal: rebase the drift gate on the new chain.

### 2026-09-28 — implemented

- **Already half done by `nf-core/default-flip`:** v8's `render` hashed the fixed decode
  over the old five pixels. That left two things: where the hash stops, and the vector.
- **The stage-goldens premise no longer holds.** "Everything downstream of the decode is
  IEEE-only" (2026-09-23) predates the look and display black. At defaults the chain now
  calls `powf` (look contrast), `log10`/`log2`/`powf` (highlight desaturation) and
  `log2`/`exp2` (display black), each windowed in `chain_golden`. Hashing through fit
  range would stack them with no window.
- **User decision: `render` stops at scene correction's input** — the decode plus the NC
  film RGB v1 → ACEScg mapping (IEEE f64, so the matrices are covered at no portability
  cost). The rendering stages' default values are `recipe`'s, their arithmetic
  `chain_golden`'s. Stated gap: a golden can be recaptured without a bump.
- **The vector: only minimum-window samples.** Measured with `reachable_window` at the
  shipped defaults. Up to about a third of a stop below the base every channel sits at 1 ULP (the
  final `powf` alone); midtones reach 4–6, dense highlights 5–9, the old out-of-range
  and floored pixel `[1.8, -0.2, 0.0]` reaches `[1, 30, 25]`. The new vector is the base,
  above it, `[0.85, 0.5, 0.38]`, `[0.8, 0.47, 0.4]`, `[0.7, 0.45, 0.35]`, and
  `the_fingerprint_vector_sits_at_the_minimum_decode_window` asserts it stays so, and
  that this host decodes each to the correctly-rounded value. The price: the scan floor
  is now the decode golden's alone.
- **User decision: v8's `render` refreshed in place** (`d44927104451d581` →
  `f51d3397c7364160`), not a v9. No default pixel moved; a v9 would need a contrived
  distinct `behavior` string and would make `pipeline_version_warning` claim the output
  will not match. The table's docs now name this as the second sanctioned in-place edit.
- **Falsifiability, by hand, reverted:** a mapping-matrix entry +1e-7, `MID_ABOVE_BASE`
  +1 ULP, and blue `DENSITY_SCALE` 0.73 → 0.731 each red the gate, its golden and its
  "actually detects" test. Linearization 1.8 → 1.85 is the standing perturbation test.

### 2026-09-28 — review fixes

- The gate's own failure messages and the v1 note had said `recipe` was the only
  in-place edit; they now defer to `PIPELINE_FINGERPRINTS`' rule.
- `the_render_fingerprint_hashes_every_pinned_value` pins the hashed text to the golden
  bits, so the mapping half cannot silently drop out of the hash.
- The third-of-a-stop sample's green moved 0.5 → 0.47, which had duplicated the
  near-base sample's green. It is still at the 1-ULP window.
- `docs/design-spec.md` and `docs/design-update.md` had still described the gate's old
  scope.
- **Not done:** re-adding dense samples (the coverage the old vector had away from the
  base). They cannot be made portable, which is this task's premise.

### 2026-09-28 — done

- **Landed:** v8's `render` = `f51d3397c7364160`, over the decode and the ACEScg
  mapping; `base` and `recipe` unmoved. Local gates green (aarch64 macOS); **x86_64
  agreement is this PR's CI to show** — if it reds on `golden_default_render_is_bit_identical`,
  change the offending sample, not the decode.
- A second review (`ship:diff-reviewer`) fixed: the gate's failure message now ties an
  in-place refresh to *what the fingerprint hashes* changing, never to the golden passing
  (a recaptured golden passes too); the samples' stop distances were overstated; the
  `core` and `nf-retire` Epic summaries still stated the old rule. Codex was skipped (out
  of credits).
- **For dependents:** a task that moves a decode default (`nf-calibration/offset-question`)
  bumps the version and recaptures `DEFAULT_FILM_BITS`/`DEFAULT_ACES_BITS` with the
  new row. A rendering-stage default moves only `recipe`.

## stage-goldens

**Status:** done (2026-09-23)
**Updated:** 2026-09-23

- 2026-09-19: created with the new-flow plan. Goal: goldens for the new stages.

### 2026-09-23 — implemented

- **Where:** `src/pipeline/chain_golden.rs` (`#[cfg(test)]`), written fresh, with its
  own vectors and its own `reachable_window`. `stages::golden` is untouched: it pins
  the legacy path and retires with it, and `golden::pixels()` stays with the historical
  fingerprint rows. **User decisions:** per-stage vectors *and* one threaded vector;
  `stages::golden` left alone; the fingerprint samples noted here rather than built.
- **Entry without `simple`:** `FilmRgbImage::fixture` (`#[cfg(test)] pub(crate)`) places
  exact film-RGB values at the mapper's input. It is the one fixture
  `nf-retire/sigmoid-and-simple` can move the `Reconstruction::Simple`-over-a-pre-inverted-scan
  helpers onto (`chain`'s and `scene_correction`'s `aces_from` among them). An
  `AcesCgImage` still has no fixture: every downstream vector enters through the real
  mapper, which is IEEE f64 and so bit-exact anyway.
- **Bit-exact vs windowed is decided by libm calls.** The decode (`log10`, `powf`) and
  a fractional exposure (`exp2`) are windowed. The mapping, stated/whole-stop scene
  correction, auto white balance (sorts, nearest rank, fixed-order f64 sum), look, fit
  range, fit gamut and the threaded vector are bit-exact. NaN is asserted as *a* NaN,
  never by payload.
- **Decode windows.** The captures are the correctly-rounded chain, and a test asserts
  that. Windows derived at the shipped offset:
  `[1,5,1, 5,3,3, 7,8,8, 1,1,1, 1,1,1, 4,6,6, 37,26,31]`. At `PROBE_OFFSET`:
  `[1,1,1, 1,1,5, 6,9,10, 1,1,1, 1,1,1, 5,1,1, 30,28,22]`. The widest are the
  floored dead pixels (density ≈ 6). Fractional exposure windows are 0–2.
- **For `fingerprints` (D4): no decode pixel can be proved portable to the bit.**
  That is structural, not measured: every window includes the final `powf`'s ULP. A hash through the decode therefore rests on *observed*
  cross-target agreement, and must be designed so. The downstream stages are IEEE-only,
  so they add no risk of their own. The base pixel (`D = 0`) and above-base pixels sit
  at window 1: only the final call, with nothing amplified. They are the natural
  candidates.
- **Falsifiability, each fault by hand, reverted:**

  | Fault | Goldens red | First in chain order |
  |---|---|---|
  | `MID_ABOVE_BASE` +1 ULP | decode golden and its integrity test | decode |
  | base channel transposed in the decode | decode | decode |
  | decode `+ offset` dropped | decode (probe-offset pass) | decode |
  | mapper row 1 `g`/`b` swapped | 6 goldens | working-space |
  | scene-correction gains channel-reversed | 3 scene-correction goldens + threaded | scene-correction |
  | `AUTO_WB_PERCENTILE` 0.95 → 0.9 | auto white balance | scene-correction |
  | look nudges every sample +1 ULP | look/fit-range, fit gamut, threaded | look |
  | fit-gamut row 2 `g`/`b` swapped | fit gamut, threaded | fit-gamut |
  | `chain::render` hands scene correction default params | threaded only | chain (threaded) |

  The first matrix run showed a look fault reddening the scene-correction goldens as
  well. They had been unwrapped *through* the identity stages. They now unwrap at their
  own boundary. Fit gamut cannot do the same: its input type is minted only by fit
  range. Hence the "read the most upstream red golden" rule in the module docs.
- **Stage-order faults.** A reorder does not compile (`chain`'s boundary types). The
  threaded vector is what catches wiring inside `chain::render`, as the last row shows.
- Gates green locally (rustc 1.98.1, aarch64): fmt, clippy `-D warnings`, build, test
  (864 + 245). `cargo doc` unresolved links stay at the baseline 16. **x86_64 Linux is
  CI's to confirm.**

### 2026-09-23 — review fixes

- The threaded vector now also pins `rendered.scene_correction`. A second threaded
  run (gray-world, `WB_REGION`) pins that `chain::render` forwards the measurement
  region, and that the gains it reports are the ones it estimated.
- The conformance test now checks the host's `powf` as well as its `log10`. Every
  decode window assumes both.
- Corrected an overclaim in the module docs and CLAUDE.md. A fault *can* red goldens
  below its stage, but the downstream vectors bypass the decode and scene
  correction's multiply, so a green one says nothing about those. The threaded
  vectors start at ACEScg. The decode→chain hand-off and the recipe→`DecodeParams`
  wiring are the orchestrator's, and stated as uncovered here.
- `FilmRgbImage`'s "sole constructor" docs now name the test fixture.
- **Not done:** sharing one window harness with `stages::golden`. That module
  retires with the legacy path, and the two differ on purpose: this copy renders the
  neighbours in correctly-rounded f64, so a window never depends on the host's `powf`.

### 2026-09-23 — done

- **Landed:** `pipeline::chain_golden` has 10 tests.
  - The decode is windowed at two offsets, with a test checking that the captures
    are correctly rounded and that the host's `log10` and `powf` conform.
  - Pinned bit for bit: the mapping, scene correction (stated, and both auto modes),
    the look/fit-range identities and fit gamut.
  - Fractional exposure is windowed.
  - Two threaded vectors run through `chain::render`, one stated and one auto with a
    region.
- **Verified:** the fault table above, and local gates green (aarch64 macOS). A Codex
  review and `ship:diff-reviewer` found only doc wording, now fixed. x86_64 is left to
  this PR's CI.
- **For dependents:**
  - `nf-retire/sigmoid-and-simple` should move the `Simple`-over-a-pre-inverted-scan
    helpers onto `FilmRgbImage::fixture`.
  - The first look control under `nf-look` (the look's knobs land one task per
    control since `nf-look/stage` closed) and `nf-display-stages/fit-range` replace
    `golden_look_and_fit_range_are_bit_exact_identities` with captured vectors, and
    recapture `THREADED`/`THREADED_AUTO`.
  - `nf-display-stages/fit-gamut` recaptures `FIT_GAMUT_P3` and the threaded vectors.
  - `nf-calibration/offset-question` recaptures `DECODE_DEFAULT`. `PROBE_OFFSET` stays
    as the non-zero witness.

## benchmark-set

**Status:** done (2026-10-01)
**Updated:** 2026-10-01

- 2026-09-19: created with the new-flow plan. Goal: a benchmark set for the new flow.

### 2026-10-01 — implemented

- **The `fixtures` set:** one HDRi case per ready `destination::ROWS` row (13) with all
  four axes stated, plus `film-master`, `--rendering direct` and `default` (no output
  flag, so it tracks what a user gets). HDR 48-bit, the other input format, runs as
  `default` and `film-master`. 18 cases, about 12 s for two runs on a debug build.
  Every depth a record can carry (`u8`, `u10`, `u16`, `f32`) is exercised. Each one
  already reported `output_stats.mean`, `loss` and a `chain.destination` that
  `depth_for_destination` maps, so the diff logic did not change.
- **User decisions:**
  - **Per-interface blocks.** A case carries `destination` and/or `preset`, each
    holding that interface's `args` and `recipe`, and runs on a build only when it has
    that build's block. Before this, a missing `preset_args` meant "no flags", which
    would render the reference's default preset under the case's name. Benchmark
    manifest schema 2. Unknown case and block keys are refused, so a v1
    `destination_args` cannot be silently ignored. The record lists skipped cases in
    `not_run` (record schema 3), and `diff` reports them as `not-run`, not `missing`.
  - **`rolls` runs on the reference too.** The destination block keeps the frozen
    recipe. The preset block restates its Dmin as `--film-base`, and a test keeps the
    two equal.
  - **The coverage gate is a Rust test**
    (`destination::tests::every_ready_row_has_a_benchmark_case`). It reads
    `benchmark.json` and asserts that the fixtures cases' stated axes equal the ready
    rows. Falsified by hand: dropping the HLG AVIF case reds it.
- **The reference arm states `--preset sigmoid-knees`** (the README's pinned config);
  the holding set had omitted it. The exception is `film-master`, which the reference
  refuses with any `--preset`. Preset map: `display-p3`, `compatibility`,
  `hdr-linear-tiff`, `hdr-pq-tiff`, `hdr-hlg-tiff`, `hdr-pq`, `hdr-hlg`,
  `gain-map-hdr`, `film-master`. No reference equivalent: Adobe RGB SDR, linear
  P3/sRGB/Adobe RGB, the sRGB gain map, `direct`.
- **Verified** (x86_64 Linux, rustc 1.97.0; reference `reserve` @ `0da32d0` built by
  `build.sh` here, release):
  - HEAD vs HEAD: `identical: true` on 18 cases. A CI test
    (`TestFixturesSetEndToEnd`) now runs exactly this on `target/debug/hanten`.
  - Reference vs reference: `identical: true` on 12 cases, 6 `not_run`.
  - Reference vs HEAD: all 12 paired cases differ at the same depth, so a pairing
    compares the pipeline and not the container. The new chain's encoded means are
    about 0.09–0.24 lower; the film master's are higher by 0.07–0.25.
    The P3 gain map's delta matches the P3 TIFF's, so the reference's gain-map base is
    Display P3 too.
  - Not verified here: the `rolls` set, which needs `../nc-assets`.

### 2026-10-01 — review fixes

- **The coverage test names its failures:** a case naming a row twice, or stating
  an axis as `--gamut=srgb` or not at all, panics with the case's name rather than
  being dropped from the set.
- **`the_benchmark_default_cases_pair_with_the_default_destination`:** the `default`
  cases pair the unset destination with the reference's `display-p3`. A default that
  moved within the 16-bit TIFF (to Adobe RGB, say) would keep the depth and read as a
  pipeline change, so the test pins the pairing.
- **The roll recipe test compares the whole recipe** with what the preset block
  restates (Dmin and SDR P3 TIFF), so a knob added to a frozen recipe cannot reach
  only one arm.
- **Documented:** the preset blocks run on the reference alone (a v6/v7 build
  refuses `sigmoid-knees`), and the diff report shares the record's schema 3 and its
  `not-run` status.
- **Not done:** rewording the task file's "holding set" design bullet. A task file
  records the work as planned.

### 2026-10-01 — done

- **Landed:** `benchmark.json` schema 2 (per-interface blocks), run records schema 3
  (`not_run`). The fixtures set covers every ready destination, the film master,
  `direct` and the default on both input formats. The two gates are a Rust coverage
  test and an nctool test that runs the set twice on the debug binary, expecting a
  zero diff.
- Gates green locally (rustc 1.99.0, x86_64 Linux): fmt, machete, clippy, build, doc,
  nctool (474), test (651 + 233). The `nc-reviewer` pass found only Lows, fixed above.
  Codex is not installed in this environment.
- **For dependents:** `analysis/display-acceptance-harness` reuses the fixtures case
  list. One case per ready row is enforced by
  `destination::tests::every_ready_row_has_a_benchmark_case`, so a row turning ready
  (the SDR JPEG) needs a case, and a preset block if the reference wrote it.

### 2026-10-01 — code-review fixes

- **The default-pairing test pinned nothing new.** It repeated
  `nothing_stated_is_the_display_p3_sdr_tiff` and never read `benchmark.json`. It is
  replaced by `every_benchmark_preset_block_writes_its_cases_destination`: each
  fixtures preset block's `--output-preset` must write the destination its case's
  `destination` block resolves to, from a table of the reference's (frozen) presets.
  Falsified: `hdri-default` paired with `compatibility` reds it.
- **The `rolls` set's keys and blocks were never validated in CI**: resolving stopped
  at the missing asset manifest. The shipped-benchmark test now resolves it against a
  stand-in asset root, so a misspelt block key reds it.
- An absolute `recipe` path is refused, as its message already said.
- `INTERFACES` now lives in `manifest`, beside `output_interface`, which returns it.
- The README no longer claims every destination runs on both input formats.
- **Kept: `default` beside `hdri-sdr-native-display-p3-tiff`.** They render the same
  thing today, but `default` follows the product default when it moves; the preset
  test then forces its reference pairing to move too. The cost is one 502×462 render
  per run.

## film-rgb-export

**Status:** done (2026-09-30)
**Updated:** 2026-09-30

- 2026-09-19: created with the new-flow plan. Goal: export the pre-matrix film rgb.

### 2026-09-29 — implemented

- **The matrix has not moved**, so the task still has content: no task plans moving the
  3×3 into scene correction, and `working_mapping` still runs in reconstruction.
- **User decisions:** `--export-film-rgb PATH` is an **operational flag, not a recipe
  key** (like `--report`), so `DecodeParams` stays the `reconstruction` section field for
  field and `--dump-params` / the `recipe` fingerprint did not move. `convert` only: `roll`
  has no such flag (clap exit 2). The file is an f32 TIFF with **no ICC profile**.
  `nctool metrics` gained a `film-rgb` space rather than reading it as `linear-srgb`.
- **Where:** `cli::render_frame` stages the export between `fixed::decode` and
  `map_nc_film_rgb_v1` (`io::encode::encode_film_rgb`), so it writes the decode's own
  buffer before the in-place 3×3: no new image buffer, and the memory model gains no
  term. It is committed before the primary like the IR plane, is guarded as a write
  target, and the report names it in `film_rgb_exported`.
- **Timed as `encode`, not a new `StageKind`:** a stage is a telemetry wire field, so a
  new one is a `SCHEMA_VERSION` bump and an upload-manifest change for a diagnostic flag.
- **`nctool metrics`:** every record now carries `channels` (per-channel key, stop
  percentiles and spread, in the file's own channels; additive, `SCHEMA` stays 2).
  `film-rgb` has no primaries, so its record has only `channels`, no `tone` / `color`.
- **Verified:**
  - `the_film_rgb_export_is_the_film_master_before_the_pinned_matrix`: one fixture run
    with `--film-master --export-film-rgb`, and the export sent through the shipped
    mapper is the master **to the bit**. A rendered destination exports the same film RGB.
  - Real scan (Ektar `971.tif`, base from `base.tif`, 5% inset). Film RGB vs master
    per-channel key: r−g −0.127 → −0.050 stops, b−g +0.211 → +0.190. Red's p95−p5 is
    4.96 → 4.85. The 3×3 mixes the channels, so the per-channel differences shrink,
    which is the direction it predicts. The export is measurably not a renamed master.
  - Local gates green (rustc 1.98.1, aarch64): fmt, clippy, build, doc, nctool (424),
    test (641 + 229).

### 2026-09-30 — review fixes

- **`nctool metrics roll --space film-rgb` crashed** (a `KeyError` on `tone`): it now
  refuses the space, since a roll tracks tone and colour axes, and names
  `metrics image` as the remedy.
- **`channels` is opt-in** (user decision): three more full-frame passes on every
  review-set cell was a cost for a block only this comparison reads. `film-rgb` always
  has it; any other space takes `metrics image --channels`. Its fractions are
  `non_*_sample_fraction` (one channel's samples, not pixels), and neither it nor a
  `film-rgb` record carries `bands`.
- **One f32 RGB TIFF writer** (`encode::write_rgb_f32`) for the export, the film master
  and the linear HDR TIFF. The export no longer runs the non-finite and channel-mean
  scans nobody read; it returns only the staged file.
- **Kept, with the reason:** timing under `encode` (a new stage is a telemetry schema
  bump), and staging the export before the chain (staging it after needs a copy of the
  frame, since the 3×3 runs in place).

### 2026-09-30 — done

- **Landed:** `hanten convert --export-film-rgb PATH` (operational, `convert` only) and
  `nctool metrics --space film-rgb` / `--channels`. The export mapped through the pinned
  3×3 is the film master to the bit (fixture test); on a real frame the master's
  per-channel differences are the smaller ones, as the matrix predicts.
- A second review (`ship:diff-reviewer`) found nothing at its bar. Its note about the
  comment on `encode_film_rgb` was right: a non-finite sample is counted by a film master's
  encode but *refused* by every rendered destination's chain, and the comment now says
  so. Codex was skipped (out of credits).
- **For dependents:** `roll-side-exports` owns the export from `roll`. If the 3×3 ever
  moves into scene correction, `film-master` becomes film RGB and this flag collapses
  into it.

## roll-side-exports

**Status:** done
**Updated:** 2026-10-08

- 2026-09-29: created as a follow-up to `film-rgb-export`, which made the export
  `convert`-only. Goal: per-frame film RGB and IR exports from `roll`.

### 2026-10-07 — implemented

- **User decision: the IR export is removed, not carried to `roll`.** `--export-ir`
  is a hidden flag refused in `reject_removed_flags` on both commands, and
  `input.export_ir` a `RETIRED_KEYS` entry; its `null`, which every older dump
  wrote, is dropped by `strip_retired_nulls`. Gone with it: `encode::export_ir`,
  `Report.ir_exported`, roll's `reject_roll_unsupported` and the memory model's
  u16 IR staging term (`RunProfile`'s variants lost their `export_ir` field; the
  calibration rows that measured the flag stay as history).
- **User decision: the "IR plane preserved but not used" warning is dropped.** With
  no export, its only remedy would have been "drop `--strict`", so every HDRi run
  under `--strict` failed with nothing to do. Carrying the plane is now the normal
  case. `scripts/real-scan-verify/harness.sh`'s `ir` stage now fails only on the
  retired message (`input carries an IR plane`), not on the decoder's layout notes,
  and its `nctool` test follows. It was the only real-scan check that `--strict`
  promotes a warning to exit 1; that coverage is gone, and only fixture tests in
  `tests/pipeline.rs` check the promotion now.
- **Kept on the telemetry wire:** `StageKind::IrExport`, `timing_ms.ir_export` and the
  upload's `conversion.ir_exported` (now always `false`). Dropping them is a
  `SCHEMA_VERSION` bump, a contract change and a Worker change for no gain; do it with
  the next schema bump.
- **`recipe` fingerprint refreshed in place** (`f3594e984e5e431b` → `f06b2b04908795a4`):
  the default recipe lost a `null` key; no default pixel moved.
- **`roll --export-film-rgb`** (user decisions): a switch, each frame writing
  `<input-stem>_film-rgb.tiff` in its output's folder (`film_rgb_export_name`), so an
  explicit `--frames` output carries its export along. The flag takes an optional
  value only to refuse it (exit 2, before recipes load): a bare switch let convert's
  `--export-film-rgb PATH` read the path as one more input scan, failing only when that
  frame decoded. The cost: put the flag after the inputs. Each export joins
  `ensure_roll_targets_distinct` beside the outputs, and the frame report carries
  `film_rgb_exported`.
- **Verified:** `roll_exports_each_frames_film_rgb_as_convert_does` (both fixtures, each
  export byte-identical to its frame's `convert --export-film-rgb`);
  `a_roll_export_colliding_with_another_frames_output_is_refused_before_rendering`
  (case-insensitive, nothing written, with a control);
  `the_retired_ir_export_is_refused_by_flag_and_by_key_on_both_commands`;
  `a_carried_ir_plane_raises_no_warning` (HDRi with a marched holder passes `--strict`).
  The tests that used `--export-ir` as a second staged artifact now use the film RGB
  and pre-encode exports. `docs/using-nc.md` re-verified against the binary for the
  sections it touches.

### 2026-10-08 — review fixes

- **A clash's remedy depends on which two targets clashed**, not on the error text.
  Write targets carry a `RollTarget` tag, and `film_rgb_clash_remedy` names a fix only
  where one works: a frame's output against its own export → rename that output;
  against another frame's export → rename it or separate the folders; two exports
  (one scan listed twice) → separate folders; an input or `--report-file` clash → no
  remedy. The first cut matched message text and advised moves that cannot work.
- **Bare `--export-ir`** reaches the removal message (`num_args = 0..=1`, like the
  other removed value flags) instead of clap's "a value is required".
- Known costs of the refused-value switch, kept: roll's usage line shows
  `[<NONE>]`, and a single scan placed right after the flag gets clap's
  "missing `<INPUTS>`".

### 2026-10-08 — done

- Landed as above. Gates: fmt, machete, clippy, build, doc, nctool, `cargo test`.
- **For dependents:** nothing reads the decoded image after `fixed::decode` now;
  releasing it early is filed as `nf-core/release-decoded-image`. The IR plane has no
  observable consumer, so `nf-core/buffer-strategy`'s positive "the plane arrives"
  check is the only planned guard that it rides the chain uncoloured.
