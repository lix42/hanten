# Hanten — nf-verification Progress Log

Execution log for the `nf-verification` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

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
- **A `--new-flow` cell cannot yet go in a review matrix**: the generator always passes
  `--output-preset`, which `--new-flow` refuses. Render that side by hand until
  `nf-destinations/preset-set`.

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

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: rebase the drift gate on the new chain.

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

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: a benchmark set for the new flow.

## film-rgb-export

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: export the pre-matrix film rgb.
