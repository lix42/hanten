# Negative Converter — Unassigned Progress Log

Log sections the epic migration could not attribute to any task — planning notes,
review triage, write-ups for tasks that no longer exist, and execution records
that were nested under another heading in the flat log (so they moved with their
parent section rather than with their own task). This is a parking lot, not a
category: when a section clearly belongs to an epic, move it into that epic's
file. (Condensed 2026-09-13: sections whose every finding now has an owning task
are reduced to an index; the pre-condensation text is in git history.)


## External review triage — 7 findings → 7 tasks (2026-07-18, docs-only, uncommitted)

An external code review of the Step-1 codebase produced seven findings, each
**verified against the actual code** (`tiffinfo`/`exiftool` on the real
`../nc-assets` scans, `cargo build`, source reads) before being turned into a
tracked task rather than fixed in place (the working tree held only doc edits).
Every finding now has an owner; where it stands on 2026-09-13:

| finding | task | status |
|---|---|---|
| input ICC → working space (`InputColor::Auto` ≡ `Linear`; all 26 real scans carry no embedded ICC or colorimetry tags — raw `Gamma=1` Plustek/SilverFast) | `input-color-management` → **deleted** (#42, 2026-07-21), superseded by `io/input-data-semantics` | done — the automatic input-ICC transform was replaced by explicit transfer/meaning resolution; see `progress/io.md` |
| density param bounds + degenerate-output (finite all-black underflow) warning | `algo/density-safety-bounds` | open — the task file carries the full evidence, including a second underflow site found 2026-07-27 |
| artifacts written straight to final paths; sidecar-fails-after-primary orphans a TIFF | `io/transactional-output-writes` | done |
| 4 GiB decode limit guards only the u16 buffer while peak is a multiple | `io/memory-preflight` | done — see the corrections below |
| strip/tile streaming, **evaluate-first** (STEP 0 gate) | `io/streaming-tiled-io` | open — still a conditional GO on the numbers below |
| three unused crates (`image`, `kamadak-exif`, `palette` — verified `cargo build --all-targets` succeeds without them) + duplicate `Algorithm` enum | `core/dependency-hygiene` | open — all three crates are still in `Cargo.toml` |
| doc-accuracy fixes + license / metadata / platforms / packaging | `core/release-readiness` | open — README status and the "two algorithms" line were fixed along the way; the research report's PUA-wrapped `citeturn` tokens (237 spans, invisible to plain grep) are still there; no `LICENSE`, no Cargo release fields |

Deferred / not created: the cheaper honest-default option for input colour
(folded into the input task, then superseded with it).

### Corrections to the parked memory-safety review framing — 2026-07-27

`io/memory-preflight` shipped; its narrative is in `progress/io.md`
`## memory-preflight`, with the peak re-measurement in
`docs/reports/real-scan-verification.md`. Three corrections to the triage's
framing, kept because `io/streaming-tiled-io`'s STEP 0 reads these numbers:

- **The pre-fix "~24 GiB / three images" figure was scoped to a hypothetical 4
  GiB-u16 input, not to anything real.** The largest real asset (`largest.tif`,
  74.65 MP HDRi) measured **3.808 GB** pre-fix and **3.146 GB** after, and the
  standard 18.66 MP frame **975 MB → 681 MB** (decimal, as `time -l` reports). The
  honest headline is "unbounded", not "24 GiB".
- **After the no-copy fix the peak moved from render to *encode*** (decoded image
  held for `--export-ir` + rendered image + the u16 quantize buffer: 38 B/px vs 32
  at render). `pipeline::memory` is the one place that model lives — any new
  full-frame buffer in any stage must be added there or the preflight silently
  under-approves.
- **"Count IR + clone" was necessary but not sufficient:** film-base *sampling*
  also allocates full-frame-scale buffers (`region_channels` materializes its
  rectangle unstrided into three `Vec<f32>`), so a model counting only the images
  under-estimates `inspect`/`estimate`.


## color-characterization-calibration
**Status:** superseded (2026-07-23)

Retired task id. Superseded by `color/optional-color-correction-profiles`
(measured neutralization as an explicitly selected, non-blocking correction; no
display task depends on it). The 2026-07-21 design decisions this section used to
record — offline fitting against controlled target data with held-out Delta E and
justified model complexity, explicit target reference coordinates/illuminant and
declared adaptation into ACEScg D60, no creative WB baked into calibration,
per-algorithm canonical input domains — live on in that task file's profile
provenance requirements and in `color/post-reconstruction-color-characterization.md`
(closed—superseded, kept as decision history).


## post-characterization-render-pipeline
**Status:** superseded (2026-07-23)

Retired task id. Superseded by `color/film-master-render-pipeline` (shipped),
which consumes typed NC film RGB v1 mapped ACEScg, renamed `scene-master` to
`film-master`, and preserves intentional film rendering rather than claiming
physical scene recovery. What that task inherited from the 2026-07-21 sketches and
still holds: the master bypass is fail-loud (any non-default downstream render
control is a usage error, never ignored), flags-win reset and resolved-report
provenance, and the shared WB → exposure → black/range display stage after the
working-space mapping. The shipped shape is recorded in `progress/color.md`.


## color-management planning — main reconciliation
**Status:** documentation reconciled
**Updated:** 2026-07-21

Reconciliation notes from rebasing the colour-management plan onto `origin/main`
after `roll-conversion` and `dmax-reference` merged. Everything they reconciled has
since shipped and is recorded under its own task (`progress/core.md`
`## roll-conversion`, `progress/film-base.md` `## dmax-reference`,
`progress/output.md` `## sdr-display-rendering`). One correction worth keeping:
`sdr-display-rendering` returns rendered-linear destination pixels plus resolved
metadata, never transfer-encoded pixels; the transfer encode happens in the
destination-output stage, and gain-map construction consumes the pre-transfer
rendition — this removed a double-encoding ambiguity in the original plan.

### Real-scan core verification — executed 2026-07-22 (task: real-scan-verification)

Full matrix run against the user's five real rolls (Ektar, phoenix, Portra160,
Portra400, Portra400-leica-flaw) on the compiled release binary. Derived numbers
only — no sample pixels read into context. Full write-up + numbers:
[`docs/reports/real-scan-verification.md`](../reports/real-scan-verification.md);
rerunnable harness + frozen recipes under `scripts/real-scan-verify/` (see its `README.md`).

- **All assets are HDRi** (`ir_present: true`; IR carried in the transparency-mask
  IFD), standard frame 5184×3599 ≈ 18.66 MP. Scanner Plustek 8300i / SilverFast 9.2.9.
- Per-roll `Dmin` (unexposed frame) + `Dmax` (fully-exposed leader) measured from a
  holder-free center-40% region, frozen to recipes. 4/5 rolls clean.
- Matrix: inspect ✅ · estimate ✅ (`--auto-base` fails loudly on every frame per the
  holder layout — correct) · convert 16-bit+float ✅ (float byte-lossless; u16 clips
  4.8–10.3% high) · IR export + `--strict` ✅ · determinism byte-identical ✅ ·
  resource ✅.
- **Resource / streaming STEP 0 input:** measured peak **~930 MiB @ 18.66 MP
  (~50 MiB/MP)**, ~1.6 s wall — ~1.5× the design's ~600 MB model (model omits the
  carried IR plane + `to_output` clone). Target = 8 GB M3 MacBook Air (2024),
  ~4–5 GB usable. Assumed 4× worst case ⇒ ~3.7 GiB (4× MP) to ~15 GiB (4× per side)
  ⇒ **`memory-preflight` gate required; streaming a conditional GO** pending
  post-preflight re-measure and the true input envelope.
- **Follow-ups:** (1) default 16-bit highlight clipping → display-output roadmap;
  (2) Harman Phoenix dense base trips the `Dmax ≳1.0` floor + base-uniformity check
  → candidate new task (per-stock/dense-base Dmax handling); (3) widen
  `memory-preflight` sizing model to count IR + clone. No hard defects.

(Kept verbatim: `progress/analysis.md` points here by heading for
`real-scan-verification`'s execution record. It belongs in that file; moving it is
a rename with a link fix, deferred.)
