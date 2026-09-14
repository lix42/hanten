# Negative Converter — io Progress Log

Execution log for the `io` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status (the checkboxes);
this file is the narrative beside it.

One `##` section per task in this epic, named by the bare task name (the part
after the `/`). Read this whole file before starting a task in this epic, and
read other epics' `Epic summary` sections when you depend on them. Append
entries — don't rewrite earlier ones. (Consolidated 2026-09-13: done tasks are
summarized; the material that open tasks still need is kept in full.)

## Epic summary

What other epics need to know about `io`:

- **`decode_within(&Path, budget_bytes) -> (LinearImage, DecodeInfo)`.** HDR vs
  HDRi is detected **structurally** (extra IFDs), not from metadata —
  `Silverfast:HDRScan="Yes"` appears on both. Real full-res scans have **three**
  IFDs (RGB, a reduced-resolution preview, then the IR plane), so the decoder
  scans all pages and skips reduced-resolution previews. Samples are 16-bit
  unsigned, normalized `/65535`, treated as linear. A **gray primary** (`Gray(16)`
  in IFD0, real Ilford HP5 scans) is refused today — `io/gray-primary-decode`.
- **IR provenance matters.** `LinearImage::ir_verified` records whether the IR
  plane carried the `NewSubfileType=4` marker or was accepted by shape alone.
  `film-base/ir-holder-detection` consumes IR only when verified. Anything else
  that starts *acting* on IR must check this flag, not just `ir.is_some()`.
- **Input semantics are two independent axes** (`pipeline/input_semantics.rs`):
  `input.transfer` (linear / unknown) and `input.meaning` (scanner-device /
  colorimetric / unknown), resolved from the **SilverFast XMP packet** (TIFF tag
  700: `Company="LaserSoft Imaging"` + `HDRScan="Yes"`, with `Gamma` feeding
  transfer). Only `Linear` + `ScannerDevice` may convert (else exit 4);
  positive-mode scans (`Negative=No`) are rejected loudly. **The provenance rule
  is grounded on exactly one scanner/software combination** (Plustek OpticFilm
  8300i + SilverFast 9.2.x) — re-validate it before trusting it on other sources.
  Embedded ICC is recorded but never applied. Neither axis says anything about
  **absolute density** — that is `io/scanner-density-calibration`.
- **The encoder is where clamping happens, and it counts the loss.** Colour and
  algo stages pass values through unclamped; `encode` returns an `EncodeReport`
  (`clipped_low`/`clipped_high`/`non_finite`) that the orchestrator turns into
  report warnings. Never clamp earlier, and never silently. f32 output is written
  verbatim (values > 1.0 preserved); u16 clamps and rounds. Non-finite samples
  are counted at **both** depths — don't launder `NaN` into a finite value in a
  stage.
- **Every artifact is written to a same-directory temp, fsynced, then renamed**
  (`io/transactional-output-writes`, 2026-08-04). `io::staged` owns the pattern;
  every writer in `io` goes through it. What other epics can rely on: **no
  truncated file ever appears at a final path**, overwrite is **atomic replace**,
  and one conversion's IR + primary + sidecar are all staged before any is
  renamed, so a later failure leaves *no* primary output. What they must **not**
  assume: multi-file atomicity — `rename` is atomic per file. If you add an
  artifact to `convert`, stage it and push it onto `pending` *before* the primary,
  which is deliberately committed last.
- **Peak memory is gated before decode** (`io/memory-preflight`, 2026-07-27).
  Every command that decodes runs `memory::preflight` on a metadata-only
  `io::decode::probe` and fails with **exit 6** (`NcError::Resource`) over budget
  (fixed 6 GiB default; `--max-memory` overrides — operational, not a recipe
  key). On **`roll`** the gate runs per frame; a rejection is that frame's error
  and the roll exits **1**. `pipeline::memory` is the one sizing model, and it is
  now **per output profile**: `Convert` (the TIFF paths: decode 18 · film-base
  16+12·s · render 32+12·s · encode 38+12·s B/px, peak at **encode**),
  `UltraHdrV1` / `GainMapHdr`, `HdrAvif`, `HdrLinearTiff`, `HdrCodedTiff` and
  `SdrTiff` — the last four peak at **render**. Which phase peaks is per profile;
  read it off `memory`'s `which_phase_peaks_is_per_profile_and_measured_not_assumed`.
  The `12·s` sampling term rides into every later phase because freed pages stay
  resident. Anything that adds a full-frame buffer to a stage **must** update the
  model by hand — no test compares it against the code — or the gate silently
  under-approves. A new preset adds and calibrates its own profile across **two**
  frame sizes before activation.
- **`color::to_output` consumes and returns the image** — it transforms the very
  buffers it was handed. Real peak on the 74.65 MP scan: 3.808 GB → 3.146 GB,
  byte-identical output. Don't reintroduce a stage-local copy of a full image
  without counting it in the model.
- **Note:** the 2026-07-21 log entries for `color/scanner-profile-before-density-experiment`
  are stranded at the tail of the `input-data-semantics` section below (they lost
  their heading in the flat log before the epic split). They are kept there under
  a labelled sub-heading.


## silverfast-decode

**Status:** done (PR #8, 2026-06-25; three-IFD fix in PR #10, 2026-07-01)

`io/decode.rs` reads SilverFast HDR (48-bit RGB) and HDRi (64-bit RGB + IR)
TIFFs into a linear `f32` `LinearImage` plus a `DecodeInfo` (format, dims,
channels, bit depth, `ir_present`, make/model/software, XMP, warnings) that
`inspect` and the report surface directly.

**On-disk layout (reverse-engineered from the user's real scans; no public
spec).** Kept in full because `io/gray-primary-decode` extends exactly this
recogniser:

- Uncompressed little-endian **ClassicTIFF**, `PlanarConfiguration=1` (chunky),
  **no `SampleFormat` tag** ⇒ 16-bit unsigned, normalize `/65535`, treated as
  linear. No BigTIFF seen (a 159 MB full-res file is far under the 4 GB limit).
- **HDR (48-bit):** one IFD — `SamplesPerPixel=3`, `BitsPerSample=16/16/16`,
  `Photometric=RGB`, `NewSubfileType=0`.
- **HDRi (64-bit), small exports:** two IFDs; IFD1 is the IR plane —
  `SamplesPerPixel=1`, `BitsPerSample=16`, `Photometric=BlackIsZero`,
  `NewSubfileType=4`, same W×H as IFD0.
- **HDRi, full-res scans (5184×3600):** **three** IFDs — IFD0 RGB, IFD1 a
  reduced-resolution RGB preview (`NewSubfileType` bit 0, 1470×1021), IFD2 the
  full-res IR plane (`NewSubfileType=4`). The decoder scans all remaining IFDs,
  skips a page only when it is *both* flagged reduced-resolution *and* smaller
  than IFD0 (a full-res IR plane with a stray bit 0 still reaches IR validation),
  and validates the first non-preview page as IR.
- **HDR vs HDRi is structural** (`more_images()`), not metadata:
  `Silverfast:HDRScan="Yes"` is on both variants.

**What is load-bearing in IR validation** (the gray-primary task must keep it):
dims equal to IFD0, `Gray(16)`, `PhotometricInterpretation=1` (BlackIsZero —
`colortype()` reports `Gray(16)` for WhiteIsZero too and the crate would silently
invert it), `NewSubfileType=4` else accepted-by-shape with a warning
(`LinearImage::ir_verified = false`).

**Error contract.** Unreadable/parse/IO → `NcError::Decode` (exit 3);
recognized-but-unhandled layout (non-16-bit, wrong channel count, planar
multi-sample, IR dim mismatch, non-gray IR) → `NcError::Unsupported` (exit 4),
via `tiff_err` mapping `TiffError::UnsupportedError` → `Unsupported`. Planar
(`PlanarConfiguration=2`) is refused rather than silently dropping G/B — the
`tiff` crate's `read_image()` returns only the first plane. `PlanarConfiguration`
read errors surface as `Decode`. Metadata tags are read **before** `next_image()`.

**Decode limits.** The `tiff` crate's default caps `read_image()` at 256 MiB;
raised to the 4 GiB classic ceiling, and since `memory-preflight` capped at
`min(4 GiB, budget)` (see below). Fixtures: `tests/fixtures/hdr-48bit.tif` (IR-free)
and `hdri-64bit.tif` (carries IR) — the second makes every `--strict` run on it
non-zero via the "IR preserved but not used" warning.


## tiff-encode

**Status:** done (PR #9, 2026-06-30)

`io/encode.rs`: `encode(image, &OutputParams, Option<&[u8]> icc, &Path) ->
EncodeReport`, `export_ir(image, depth, &Path)`, `write_sidecar(output_path,
recipe_json)` (`out.tiff` → `out.tiff.json`). Every `&Path` entry point wraps a
`*_to_writer<W: Write + Seek>` core so tests round-trip through a `Cursor`.
Since `transactional-output-writes` all three return/use a `Staged` (see below).

Durable decisions:

- **u16 quantization:** `v.clamp(0, 1) * 65535` then `f32::round`
  (half-away-from-zero); `NaN` forced to 0. **f32:** written verbatim, no clamp.
- **`EncodeReport { total_samples, clipped_low, clipped_high, non_finite }`**
  (`#[must_use]`): `clipped_*` = finite out-of-`[0,1]` values clamped on the u16
  path; `non_finite` counted at **both** depths. The encoder only surfaces; the
  orchestrator folds it into the report and honours `--strict`. `export_ir`
  discards its report behind a `debug_assert!(!any_loss())` because IR is
  decode-normalized and carried untouched.
- **BigTIFF `Auto`:** promote when `w*h*channels*bytes + ICC bytes + 1 MiB` exceeds
  `u32::MAX`; classic vs big are different `TiffKind` types, so one generic
  `encode_planar<W, K, C>` helper is monomorphized by a `match (depth, big)`.
- ICC embedded via `Tag::IccProfile` (34675) before `write_data`.
- Encoder errors map to `NcError::Write` (exit 5). The `tiff` encoder never
  flushes, so the writer is borrowed and flushed explicitly (now
  `io::staged::flush_surfacing_errors`) — a `BufWriter` drop-flush would swallow
  a disk-full on the last block.
- `export_ir` checks `image.ir` exists *before* creating the file, so a no-IR
  failure never truncates a target the user pointed `--export-ir` at.


## input-data-semantics

**Status:** done (PR #43, 2026-07-22)

Two-axis input semantics (`input.transfer` / `input.meaning`) resolved by the
pure, table-tested `pipeline::input_semantics::resolve`, with provenance keyed
on the SilverFast XMP packet. Supersedes the old `input.color` / `--assume-linear`
/ `--input-profile` surface: the first two are pinned migration errors,
`--input-profile` is a loud `Unsupported` (exit 4) **reserved for
`color/scanner-profile-before-density-experiment`**.

**Contract** (what the open tasks build on):

- **Types.** `TransferAssertion { auto, linear }` ⇒ `input.transfer`;
  `MeaningAssertion { auto, scanner-device, colorimetric }` ⇒ `input.meaning`.
  Resolver output: `TransferDescription { linear, unknown }`,
  `MeasurementMeaning { scanner-device, colorimetric{reference}, unknown }`
  (serialized as a flat kebab-case string; the reference rides in
  `InputColorReport::meaning_reference`), evidence records `InputEvidence { axis,
  kind, detail, provenance?, displaced? }` with `EvidenceKind { user-assertion >
  structural > descriptive > embedded-icc > default }` — contradiction-aware,
  not a blind precedence pick. `ContainerColorFacts { raw_mode, gamma:
  GammaFact, embedded_icc }` is the decode → resolver hand-off.
- **Rules.** `auto`/`auto` never errors (ambiguity ⇒ `Unknown`); an explicit
  assertion contradicting authoritative structure is a usage error (exit 2).
  `require_convertible` passes only `Linear` + `ScannerDevice`, else exit 4.
  SilverFast raw-mode structure proves **both** axes; gamma proves **only**
  transfer; a non-linear or **malformed** gamma (`GammaFact::Malformed`, e.g.
  locale `"2,2"`) contradicting raw-mode ⇒ transfer `Unknown` unless
  `--input-transfer linear` overrides (recorded as displaced). nc never guesses
  comma decimals. Embedded ICC (tag 34675, `DecodeInfo.embedded_icc`,
  `#[serde(skip)]`) is informational device-characterization evidence, never
  applied, never establishes an axis; the report carries a safe lcms2 summary.
- **Provenance is the XMP packet, not `Software` or IR presence** (an
  adversarial review found both misclassify processed exports and generic
  RGB16+Gray16 multipage files). Tag 700 → `parse_silverfast_xmp` (roxmltree,
  `Silverfast:` attributes, namespace URI `LSI/`) → `SilverfastXmp { company,
  hdr_scan, gamma, negative }`. `is_silverfast_raw_mode()` = `Company=="LaserSoft
  Imaging" && HDRScan==Some(true)`; `is_silverfast_positive_mode()` fires only on
  an explicit `Negative=No` (`yes_no` returns `None` for anything unrecognized, so
  an odd value never masquerades as a positive). Absence is silent; a present but
  unrecognizable packet, a read error, or a malformed gamma each push a decode
  warning rather than dropping provenance silently.
- **Wiring.** `convert_frame` resolves + gates after decode, before film-base;
  `inspect` reports without gating; `roll` frame reports carry `input_color`, and
  `reject_roll_unsupported_input` rejects the pre-decode-decidable
  `input.meaning = colorimetric` before the frame loop. CLI-vs-recipe provenance
  is threaded at flag granularity (`InputFromCli`).
- **Extension points for dependents.** `ColorReference` and `RawMode` are where
  colorimetric spaces and non-SilverFast raw modes go; the resolved
  `InputColorMetadata` (with retained `embedded_icc`) is the hook for a
  scanner→working characterization. `ContainerColorFacts.gamma` is `Absent` on
  every real decode today — SilverFast proves linear structurally — so the
  gamma logic is exercised only by table tests and future encoded/DNG inputs.
  Verified on every real negative in `../nc-assets` (48/64-bit, full and small,
  with and without embedded ICC): all resolve scanner-device/linear; both positive
  samples hit the positive-mode error.

**Deferred follow-ups, never filed as tasks (still true 2026-09-13):**
(a) **positive-mode + embedded-ICC support** — use the `Negative` flag and the
retained ICC to convert positive-mode / ICC-embedded SilverFast scans; (b)
**re-validate provenance detection on a wider sample set** — other scanning
software, scanners, cameras, SilverFast configurations: the gate is grounded on
one scanner/software combination (Plustek OpticFilm 8300i + SilverFast 9.2.x) and
was the user's explicit request to re-examine when broader samples exist. The
F2 "unrecognized XMP" warning exists so a future namespace difference is visible.

### Stranded entries: `color/scanner-profile-before-density-experiment`

(These two entries lost their heading in the flat log before the epic split;
that task is `color`'s. Left here verbatim.)

- 2026-07-21: Split scanner-profile placement into a deferred controlled
  experiment. Compare density-first, ICC-first in a defined linear colorimetric
  space, and joint scanner+film characterization using target-patch error; do not
  lift `--input-profile` into the normal workflow without evidence.
- 2026-07-21: Narrowed after design review: this task now compares only raw
  density-first versus applying the same conventional scanner ICC to image and
  Dmin before density. Post-reconstruction characterization is an independent
  production track and is not blocked on this deferred experiment.


## memory-preflight

**Status:** done (PR #58, 2026-07-27)

Both halves shipped: the no-copy colour transform and the peak-memory preflight.
The model, calibration table and determinism scope live in `pipeline/memory.rs`'s
module doc; this section keeps the before/after set and the lessons that are not
in the code.

- **Measured on `samples/largest.tif`** (10368x7200 = 74.65 MP HDRi, the
  `perf-worst-case` asset), release binary, `/usr/bin/time -l` peak RSS,
  macOS/aarch64:

  | run | before | after |
  |---|---|---|
  | `convert` u16 | 3.808 GB | 3.146 GB |
  | `convert --output-hdr` (now `--out-depth f32`) | 3.892 GB | 2.698 GB |
  | `convert --export-ir` | 3.893 GB | 3.146 GB |
  | `inspect` / `estimate` | 1.502 GB | 1.503 GB (no render) |

  A standard 18.66 MP frame went **975 MB → 681 MB** (decimal; ~930 MiB → 650 MiB)
  — a 30% cut. Output byte-identical on all real-scan and fixture paths. Units:
  measured peaks and model outputs in **decimal GB/MB**, budgets in **GiB/MiB**.
  The u16 `convert` numbers are this log's contribution to
  `docs/reports/real-scan-verification.md`'s table.
- **No-copy transform.** `color::to_output(LinearImage, &OutputParams) ->
  Result<(LinearImage, Vec<u8>)>` transforms the buffers it was handed. Consuming
  the image rather than taking `&mut` is deliberate: `profile_icc` can fail
  *after* the transform has run, and a by-reference signature would hand the
  caller a half-converted buffer.
- **The model** (HDRi u16, `s` = sampled rectangle ÷ frame): decode 18 ·
  film-base 16+12·s · render 32+12·s · encode 38+12·s B/px. **Encode** is
  `convert`'s peak (decoded + rendered + u16 quantize, because the decoded image
  is held for `--export-ir`); **film-base** is `inspect`/`estimate`'s
  (`RunProfile::DecodeOnly`). Two full images overlap by design.
- **Three modelling lessons, each caught by review or measurement:**
  1. **Film-base sampling is not free.** `film_base::region_channels`
     materializes its rectangle unstrided into three `Vec<f32>` (12 B per
     sampled pixel; auto interior ≈ 69% of a 3:2 frame ⇒ ~24 B/px; full-frame
     region ⇒ 28). The first model omitted it and `estimate --base-region` on the
     74.65 MP frame was admitted at 1.679 GB and peaked at **2.119 GB (+26%)**
     (+43% full-frame). The interior rule lives once, in
     `film_base::auto_interior_pixels`, which sampler and model both call.
  2. **Freed is not gone.** Letting the sample *compete* (`max`) with later
     phases still under-estimated a full-frame `--base-region` `convert` by
     **10.2%** (3.743 GB measured vs 3.396 predicted). Peak RSS is a high-water
     mark and the allocator keeps the pages, so the sample is **retained** into
     render and encode — 50 B/px, reproducing the measurement to +0.3%. That is
     what pushed the default budget from 4 GiB to **6 GiB**.
  3. **Two accounting subtleties.** At decode the u16 read buffer is freed before
     the IR plane is read, so those are alternatives (`max`) over the RGB f32
     base — counting the whole decoded image gave 22 B/px and +29% on `inspect`.
     At encode the IR export buffer is summed with the quantize buffer as a
     deliberate over-count: `convert` and `convert --export-ir` measure the
     *same* peak.
- **Allowance = 15% + 128 MiB.** Worst measured overhead 12.9% (74.65 MP
  `--output-hdr`), so 15% keeps a 2.1 pp margin. Every calibration point
  over-estimates (+4.6% to +76%); an under-estimate would approve the run that
  OOMs. The fixed term is a floor: nothing is admitted under a 128 MiB budget,
  and the message says so.
- **Budget: fixed 6 GiB default + `--max-memory`; warn tier tracks RAM.** The
  hard decision is machine-independent; the **warning** compares against 70% of
  detected physical RAM (`sysctlbyname("hw.memsize")` on Darwin; `min(/proc/meminfo
  MemTotal, cgroup v2 `memory.max` / v1 limit)` on Linux, nested cgroups included,
  else `None` ⇒ no warning). Via `--strict` this is the one way the exit code can
  differ between machines (design-spec §11). Linux detection is a plain function
  with unit-tested parsers so it type-checks on macOS.
- **`--max-memory` is operational** (arg struct only; a recipe carrying
  `max_memory` is rejected by `deny_unknown_fields`; proven not to perturb bytes).
- **The gate needs a metadata-only probe.** `io::decode::probe(&Path) ->
  ImageShape` reads IFD0 dims/colortype plus an IFD walk for the IR plane and
  never calls `read_image`. Rejecting `largest.tif` under `--max-memory 2GiB`
  exits 6 in 0.29 s at **3.5 MB** peak RSS with nothing written. Tests pin probe
  and decode against each other, because a missed IR plane under-estimates by a
  third of the RGB footprint.
- **`decode` is `decode_within(&Path, budget_bytes)`**, with the `tiff` read
  buffers capped at `min(4 GiB, budget)` — the preflight is the authority, the
  cap is defense-in-depth against a header whose strip sizes disagree with the
  probed shape. Consequence: a small-but-passing budget can turn a decodable file
  into an exit-3 decode failure (documented in `using-nc.md`).
- **Exit 6** (`NcError::Resource`) is distinct from `Unsupported` (4): the input
  is fine, *this run on this budget* is not, so an agent retries with
  `--max-memory`. On `roll` the memory block lives on the frame entry, siblings
  still convert, and the roll exits **1**.
- **Reproducing** (release binary; film base arbitrary — peak RSS does not depend
  on it):

  ```bash
  BIG=../nc-assets/samples/largest.tif
  /usr/bin/time -l ./target/release/nc convert --film-base 0.9,0.55,0.42 \
      -o /tmp/big.tiff "$BIG" | jq .memory      # estimate vs measured RSS
  /usr/bin/time -l ./target/release/nc convert --film-base 0.9,0.55,0.42 \
      --max-memory 2GiB -o /tmp/nope.tiff "$BIG"   # exit 6, ~3.5 MB peak
  ```

  `memory.estimated_peak_bytes` compares directly to `time -l`'s maximum
  resident set size; pinned pairs are in
  `pipeline::memory::estimate_stays_conservative_against_the_measured_peaks`.
- **Still open from this task:**
  - **The auto-interior sample is derived, not measured.** Every calibration row
    used an explicit `--film-base`; the auto path's ~8.3 B/px is computed (74.65 MP
    `inspect --auto-base`: 1.811 GB accounted, 2.22 GB estimated) and the detector
    refuses on every real asset, so no `time -l` run has confirmed it. For
    `io/streaming-tiled-io`: that term is a future cost, but a user-sized
    `--base-region` already reaches 12 B/px today.
  - **Nothing enforces the model against the code.** The per-phase test compares
    `estimate_peak` to the numbers in the module doc; a new full-frame buffer is
    invisible until someone updates both. The film-base omission above is the
    proof it happens.
  - **Getting below two overlapping images** is `io/streaming-tiled-io`; the
    decoded image outlives the render because `--export-ir` reads it.
- Later profiles (`UltraHdrV1` 2026-07-31, `HdrAvif` 2026-08-05,
  `HdrLinearTiff`/`HdrCodedTiff` 2026-08-06, `SdrTiff` 2026-08-09, `GainMapHdr`
  2026-08-10) were added and
  calibrated by their `output` tasks; see `docs/progress/output.md` and the
  module doc's calibration table. `UltraHdrV1`'s 20 B/px `byte_staging` term is
  spent on libultrahdr's native copies and must be re-measured by
  `output/ultrahdr-dependency-externalization`.


## streaming-tiled-io

**Status:** not started
**Updated:** —

- Goal: Bound peak memory to a small working set (a few strips/tiles) instead of several whole-image buffers, by moving decode and encode toward **strip/tile streaming**: strip/tile decode, quantize-and-write encode strip-by-strip, avoiding materializing the full u16 and f32 images at once.


## transactional-output-writes

**Status:** done (PR #72–#74, 2026-08-04)

`src/io/staged.rs` (`stage` / `stage_bytes` / `Staged::commit` / `commit_all`);
`io::encode::{encode, export_ir, write_sidecar}`, `io::ultra_hdr::encode` and
`cli::write_json` all go through a `Staged`. Determinism verified against the
pre-change binary: byte-identical primary, sidecar differing only in
`meta.git_dirty`.

**Decisions the task asked to pin down:**

- **fsync each temp: yes; parent directory: no.** Directory fsync buys only
  power-loss durability for the rename itself, costs a Unix-only path, and the
  output is reproducible by re-running.
- **Overwrite is atomic replace** (`fs::rename` replaces on Unix and Windows).
  On Windows a rename can fail if the destination is held open; CI is Linux +
  macOS, so that path is **untested, not guaranteed**.
- **`--report-file` and `--dump-params` are staged individually but are not
  part of the conversion's set** — the report must land even when `--strict`
  then fails the run, and under `roll` it is roll-level. Telemetry is unchanged
  (after finalized output, best-effort).
- **The set is IR + primary + sidecar**, all staged before any rename; under
  `roll` it is per frame. The sidecar path is derived from the final image path
  (`output/output-path-suffix` must confirm it sees the completed path, not the
  stem).

**Findings worth keeping:**

- **Commit order is the fix, and a test found it.** Occupying the sidecar's path
  with a directory fails the *rename*, not the write, so with in-order commits the
  primary was already promoted — the very orphaned-primary bug. `commit_all`
  pre-checks every target for a directory blocker and rejects duplicate resolved
  targets, and the **primary is committed last**: its presence at the final path
  is what reads as "succeeded".
- **`Staged` is `#[must_use]` and unlinks on `Drop`;** `commit` takes `self` and
  puts the temp path back on failure.
- **"Replace open-and-write with a rename" silently changes at least eight
  behaviours**, all found by review of the fix rather than the original code
  (PR #72–#74). Each is now handled: an existing file's **mode** is copied onto
  the temp (hard error if it cannot be; ACLs/xattrs are not carried); a
  **symlink** target is followed (one hop hand-resolved for a dangling link) so
  the link is not replaced by a file — which also keeps the rename
  same-filesystem, and canonicalizes the directory (`/private/var/…` on macOS);
  temp names use `create_new` with a per-process OS-seeded random token so two
  processes (or stale temps from signal-killed runs) cannot collide; the suffix
  uses a truncated *prefix* of the basename so `NAME_MAX` cannot be exceeded; a
  **read-only** target is refused by opening it for write without truncating
  (skipped under root, which ignores modes); a **FIFO/socket/device** target is
  refused, and that type check runs *before* the writability probe or opening a
  FIFO blocks forever; two artifacts resolving to **one file** are rejected via
  `alias_key`, which canonicalizes the parent directory and re-joins the file
  name (lexical normalization of `sub/../ir.bin` named a *different* file when
  `sub` was a symlink — the access path stays verbatim, only comparison
  canonicalizes); a **hard-linked** target cannot be preserved without giving up
  atomicity, so `commit` reports it — conversion artifacts as report warnings
  (`--strict` promotes), `write_json` targets to stderr only, so a hard-linked
  report file cannot fail a strict render.
- **Temp cleanup holds on ordinary error paths only.** Destructors do not run when
  a signal kills the process; final-path integrity still holds unconditionally
  (the final path is never opened for writing). No signal handler or startup
  scavenging is installed.
- Two test lessons: run each new test **alone** (an order-dependent stale-temps
  test passed only because earlier tests advanced a counter), and a comment
  excusing a known-wrong shortcut deserves the same scrutiny as the code — one
  here was wrong about its own call graph.


## scanner-density-calibration

**Status:** not started
**Updated:** 2026-08-02

- Goal: Establish what a scanner's numbers mean in **absolute** density, so
  manufacturer-published densities are directly usable by reconstruction.
  `input-data-semantics` resolved transfer and meaning but not absolute
  normalisation — this closes that gap.
- 2026-08-02 (filed during `algo/reference-anchored-sigmoid` planning): two tiers,
  split by what they ask of the user. **Tier 1** uses only an unexposed frame, which
  the workflow already requires for `Dmin`: compute `−log10(scan)` per channel
  *without* dividing by `Dmin` and compare to the stock's published `D-min` (Ektar
  100: R ≈0.20 / G ≈0.56 / B ≈0.77). **Tier 2** adds a grey card or step wedge for a
  full offset+slope profile, and must stay strictly optional.
- **Known limit of tier 1, to be stated in its report rather than glossed:** one
  known density fixes a zero point, not a slope — and nc already anchors at the base
  by construction (`D = −log10(scan/Dmin)`), so a base measurement adds no new zero
  point. Its real value is supplying *three* known densities at once (one per
  channel, spanning ≈0.57 on Ektar), so a compressed spread is a slope signal. The
  irreducible ambiguity: a mismatch may be a wrong scale **or** a scanner filter
  whose spectral response differs from Status M. Report the ambiguity; don't pick.
- **A mismatch is not fatal.** Datasheets give the *relationship* between landmarks
  (mid-grey → diffuse white, Δ ≈ 0.36); a locally measured Δ gives the scale. Off
  scale just means deriving contrast from the measured Δ — a different number, same
  method. So the profile is a correction to apply, never a gate on conversion.
- Distinct from `color/scanner-profile-before-density-experiment`, which is about a
  *colour* transform before density conversion; this is about the *density scale*.
  Don't conflate them.
- 2026-08-02 (PR #68 Codex review, three findings accepted): **Tier 1 is a
  non-calibrating diagnostic, not a calibration.** `io::decode::normalize_u16` divides
  16-bit samples by 65535, so a scan value is a code-value ratio against *full scale*,
  not `I/I₀`. Scanner exposure and per-channel gains put an arbitrary offset between
  `−log10(scan)` and published `D-min`, so a perfectly linear scan can disagree with the
  datasheet for reasons that have nothing to do with scale. Absolute density needs a
  **same-settings open-gate reference measurement**.
- **The cross-channel-spread-as-slope argument was wrong and is removed.** The three
  channel readings are one point on three *different* response curves, each with its own
  gain and spectral sensitivity — not three points on one curve. A compressed spread can
  come from channel gains alone, and no channel has a second point from which a slope is
  identifiable; deriving a correction from it would corrupt colour. Slope needs a second
  known density *per channel*.
- **Tier 2 needs a calibrated transmission step wedge, not a photographed grey card.** A
  photographed card's developed density depends on illumination, exposure, processing and
  the characteristic curve, so it is not a known density and cannot pin offset+slope.
- 2026-08-02 (local Codex branch review, two further findings accepted):
  `algo/film-stock-profiles` is now a **real prerequisite**, not a coordination note. The
  verification needs the per-stock nominal `D-min` while this task's own spec forbids
  keeping a second copy — duplicating datasheet values across two modules is the exact
  silent-drift risk the `pipeline/colorimetry/` pattern exists to prevent.
  `algo/reference-anchored-sigmoid` becomes transitive through it.
- Verification no longer claims "committed fixture rolls" — **there are none.** Real rolls
  live in the machine-local, uncommitted `../nc-assets` and must be identified through
  `manifest.json` (roll + frame + `sha256`) and driven via `scripts/real-scan-verify/`;
  that half cannot run in CI and skips when assets are absent. Tier 1's *logic* is covered
  by a synthetic committed fixture so a clean checkout can still verify it.
- 2026-09-12 (**postponed, and the protocol moved**): the calibrating tiers need a
  ColorChecker bracket on two rolls that does not exist yet, so the task is parked by user
  decision. Tier 1 (the non-calibrating diagnostic) is still implementable without it, but
  tier 1 alone does not fulfil the goal, so the task now depends on
  `analysis/calibration-frame-capture` rather than carrying the shoot implicitly. The
  protocol agreed 2026-09-08 moved to that task; what stays here is *why* each requirement
  exists (neutral series vs one patch, coloured patches for the off-diagonal terms, the
  bracket's illuminant-independence, two rolls, one development batch and scan session).

## gray-primary-decode

**Status:** not started
**Updated:** 2026-08-11

- Goal: accept a SilverFast scan whose primary is 16-bit grayscale; IR page unchanged.
- Found 2026-08-11 while testing whether IR can identify the film holder without a
  declared film type. Seven Ilford HP5 frames in `../nc-assets/rolls/ILFORT-HP5-2026-08-10/`
  fail at decode: `expected 3-channel 16-bit RGB in the primary image, found Gray(16)`.
  Their `IFD2` is a full-resolution "Transparency mask" (`NewSubfileType=4`), so the IR
  plane is present and marker-verified — only the primary's channel count differs.
- **No existing task owned this.** `io/silverfast-decode` required `Gray(16)` only for
  the IR plane beside an RGB `IFD0`; `algo/bw-support` explicitly excludes input-format
  work ("16-bit RAW scan *input* is a separate concern"). So `bw-support` was blocked on
  a task that did not exist.

## positive-input-mode

**Status:** not started
**Updated:** 2026-09-13

- Goal: convert an already-positive SilverFast scan (`Negative=No`, embedded ICC)
  through the display path with no reconstruction. `input-data-semantics` detects and
  refuses it today (exit 4) and deferred this "to file formally"; a positive roll is in
  the asset set.
