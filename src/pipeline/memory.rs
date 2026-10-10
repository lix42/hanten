//! Peak-memory preflight: one sizing model for "how much will this run
//! allocate", the operational budget it is checked against, and the fail-loud
//! verdict (`io/memory-preflight`).
//!
//! ## Why this exists
//!
//! `io::decode::decode_limits` advertises a 4 GiB *input read buffer*, which
//! guards only the `tiff` crate's `u16` staging buffer. The derived peak is a
//! multiple of it, because whole-image buffers **overlap** (the `f32` image beside
//! its read buffers, the encoder's staging, a second rendition). So the run's
//! real ceiling was never checked at all. This module makes it explicit and
//! checkable *before* the first big allocation.
//!
//! ## The model
//!
//! Per-phase accounting of the full-frame buffers that are **simultaneously
//! live**, in bytes-per-pixel for the shipped 16-bit RGB + IR input (`decoded` =
//! the decoded image, `rgb32` = one interleaved `f32` RGB buffer, `ir32` = one
//! `f32` IR plane). A 16-bit TIFF destination ([`RunProfile::U16Tiff`]):
//!
//! ```text
//! decode     rgb32 + max(rgb16 read buffer, ir16 + ir32)      18 B/px
//! film-base  decoded (rgb32+ir32) + 3 f32 channel vectors     16 + 12·s B/px
//!            over the sampled rectangle
//! render     the chain's buffer (rgb32) + retained sample     12 + 12·s B/px
//! encode     rendered + u16 quantize + retained               18 + 12·s B/px
//! ```
//!
//! The chain's buffer **is** the decoded image: `algo::fixed::decode` consumes the scan,
//! drops its IR plane and writes the positive into its RGB, and every stage moves that
//! buffer on and works in place (`pipeline::chain`'s buffer rule). A 32-bit float TIFF ([`RunProfile::F32Tiff`])
//! writes it verbatim, so its encode row has no quantize term and, with nothing
//! sampled, its peak is the decode row. The gain-map JPEG
//! ([`RunProfile::GainMapJpeg`]) holds two renditions and the full-resolution gains
//! at render (`chain::render_pair`). Which phase peaks is **per profile**, not a
//! property of any category: `which_phase_peaks_is_per_profile_and_measured_not_assumed`
//! pins each one. Note a lossless-TIFF compression option would add a staging term
//! and could move a TIFF's peak to encode.
//!
//! `s` is the sampled rectangle as a fraction of the frame ([`SamplePlan`]): `0`
//! for an explicit `--film-base` (nothing is sampled), up to 1.0 for a full-frame
//! `--base-region`. The effective-area measurement (`film_base::measure_area`) is
//! `0` too: it counts 16-bit codes into a fixed ~1.5 MB histogram instead of
//! copying pixels, which the allowance covers.
//!
//! Notes on the non-obvious entries:
//!
//! - **One image at render.** The decode and every colour transform run in place.
//!   `--export-film-rgb` writes the decode's buffer verbatim before the 3×3, and
//!   `--export-pre-encode` the buffers the render already holds, so neither adds a
//!   term.
//! - **Retention: a freed buffer is still counted at every later peak.** The film-base
//!   sample is **added into** the render and encode phases rather than competing with
//!   them. This was macOS malloc's behaviour: it kept freed large blocks resident, and
//!   a full-frame `--base-region` `convert` measured **3.743 GB**, which 50 B/px (32 render + 6
//!   quantize + 12 retained sample) reproduced to +0.3% where a competing phase
//!   under-estimated it by 10.2%. [`crate::allocator`] now unmaps blocks of 8 MiB or more
//!   on free, so a full-frame sample leaves before render (measured); smaller blocks and
//!   lcms2's own still go through malloc, so the terms are kept — an over-count for big
//!   samples, the safe side. `decode`'s `u16` read buffer is a genuine alternative
//!   (`max`): it is freed and the IR buffers are allocated into the space it vacated,
//!   within one stage.
//! - **`film_base` *does* allocate a full-frame-scale buffer.** It samples
//!   rectangles, but `film_base::region_channels` materializes each one
//!   *unstrided* into three `Vec<f32>` — 12 bytes per sampled pixel — live
//!   alongside the decoded image, so a full-frame rectangle costs 28 B/px. For
//!   [`RunProfile::DecodeOnly`] (`measure-base --base-region`, which stops after
//!   sampling) that phase **is** the peak, well above decode's 18 B/px; `inspect`
//!   and a sourceless `measure-base` gather nothing, so decode is theirs. The sample is
//!   *retained* into a conversion's later phases (see the retention rule above), so
//!   sampling raises `convert`'s peak rather than being free. Those phases no longer
//!   hold the IR plane, so on an HDRi scan the float TIFF and `measure-roll` peak at
//!   film base once the sample passes a sixth of the frame (16 + 12·s against the
//!   decode's 18).
//!
//!   This note replaces an earlier claim that `film_base` allocated no full-frame
//!   buffer. That claim was false twice over: it let `inspect`/`measure-base` be
//!   admitted at 18 B/px and then exceed their own predicted peak (measured +26%
//!   on the auto-interior rectangle, +43% on a full-frame one), and the first fix
//!   — counting the phase but letting it *compete* — still under-estimated a
//!   full-frame-region `convert` by 10.2%.
//!
//! On top of the named buffers sits an **allowance** ([`ALLOWANCE_PERCENT`] +
//! [`ALLOWANCE_FIXED_BYTES`]) for what a byte-exact model cannot see: allocator
//! slack and freed-but-resident pages, the binary and its static data, lcms2
//! profiles and transforms, and the `tiff` writer's buffering. It is calibrated
//! to **over**-estimate on every measured point — a preflight that
//! under-estimates would approve a run that then OOMs, which is the failure this
//! task exists to prevent.
//!
//! **Nothing enforces this model against the code.** The test
//! `phase_totals_match_the_documented_bytes_per_pixel` compares `estimate_peak`
//! against the per-pixel figures written *in this doc*; no test observes what the
//! pipeline actually allocates. So a new full-frame buffer in any stage stays
//! invisible until someone adds it here by hand, and the gate silently
//! under-approves in the meantime. That failure has already happened once — the
//! film-base phase above was missing from the first version of this model.
//!
//! ## Calibration (macOS/aarch64, `/usr/bin/time -l` peak RSS, 2026-07-27)
//!
//! Measured on `largest.tif` (10368x7200 = 74.65 MP HDRi, the largest scan on
//! hand), a standard roll frame (5184x3599 = 18.66 MP HDRi), and the Ultra HDR
//! gain-map calibration scan (5184x3600 = 18.66 MP HDRi). Units: measured peaks
//! and model outputs in decimal GB (that is what `time -l` reports against);
//! budgets and the allowance in GiB/MiB. The calibration conversions used an
//! explicit film base, so their render/encode peaks carry no sampled rectangle.
//!
//! Every row is model-vs-measured for the *same* invocation; the model must never
//! come in under measured. The sampling rows are what the film-base phase was
//! added for — before it was modelled, the last three 74.65 MP rows here
//! under-estimated by 26%, 43% and 10% respectively.
//!
//! **Most rows measured the chain `nf-core/default-flip` removed** (`convert` u16 and
//! f32, the gain-map, AVIF and HDR TIFF presets). Their profiles are gone, so their
//! model column is history: the rows stay as the evidence behind the constants they
//! fitted — [`ALLOWANCE_PERCENT`] and the retention rule — and because the current
//! profiles count the same buffers they did (the `--new-flow` rows below matched a
//! u16 `convert` of the same frames to 0.1 MB).
//!
//! | run | model | measured | margin |
//! |---|---|---|---|
//! | `convert` u16 74.65 MP | 3.396 GB | 3.146 GB | +8.0% |
//! | `convert --export-ir` u16 74.65 MP | 3.568 GB | 3.146 GB | +13.4% |
//! | `convert --output-hdr` 74.65 MP | 2.881 GB | 2.698 GB | +6.8% |
//! | `measure-base --film-base` 74.65 MP (no sampling) | 1.679 GB | 1.502 GB | +11.8% |
//! | `measure-base --grid` 74.65 MP | 1.679 GB | 1.558 GB | +7.8% |
//! | `measure-base --base-region` (auto-interior rect) 74.65 MP | 2.217 GB | 2.119 GB | +4.6% |
//! | `measure-base --base-region` (full frame) 74.65 MP | 2.538 GB | 2.398 GB | +5.8% |
//! | `convert --base-region` (full frame) 74.65 MP | 4.427 GB | 3.743 GB | +18.3% |
//! | `convert` u16 18.66 MP | 0.950 GB | 0.681 GB | +39.4% |
//! | `ultra-hdr-v1` 18.66 MP (explicit base, no IR export) | 1.851 GB | 1.681 GB | +10.1% |
//! | `hdr-pq` 18.66 MP (explicit base, no IR export) | 1.765 GB | 1.472 GB | +19.9% |
//! | `hdr-pq` 74.65 MP (explicit base, no IR export) | 6.659 GB | 5.866 GB | +13.5% |
//! | `hdr-hlg` 18.66 MP (explicit base, no IR export) | 1.765 GB | 1.503 GB | +17.5% |
//! | `hdr-linear-tiff` 18.66 MP (explicit base, no IR export) | 1.078 GB | 0.907 GB | +18.9% |
//! | `hdr-linear-tiff` 74.65 MP (explicit base, no IR export) | 3.911 GB | 3.578 GB | +9.3% |
//! | `hdr-pq-tiff` 18.66 MP (explicit base, no IR export) | 1.078 GB | 0.906 GB | +19.0% |
//! | `hdr-hlg-tiff` 18.66 MP (explicit base, no IR export) | 1.078 GB | 0.906 GB | +19.0% |
//! | `hdr-pq-tiff` 74.65 MP (explicit base, no IR export) | 3.911 GB | (not measured) | — |
//! | SDR preset 15.55 MP (explicit base, no IR export) | 0.921 GB | 0.850 GB | +8.4% |
//! | SDR preset 74.65 MP (explicit base, no IR export) | 3.911 GB | 3.594 GB | +8.8% |
//! | `--new-flow` 14.45 MP (explicit base, no IR export) | 0.766 GB | 0.618 GB | +23.9% |
//! | `--new-flow --export-ir` 14.45 MP (explicit base) | 0.799 GB | 0.618 GB | +29.3% |
//! | `--new-flow` 18.66 MP (explicit base, no IR export) | 0.950 GB | 0.795 GB | +19.5% |
//! | `--new-flow --export-ir` 18.66 MP (explicit base) | 0.992 GB | 0.795 GB | +24.9% |
//! | `--new-flow --rendering direct` (HDR float) 16.26 MP (explicit base) | 0.733 GB | 0.596 GB | +22.8% |
//! | `--new-flow --rendering direct` (HDR float) 18.66 MP (explicit base) | 0.821 GB | 0.683 GB | +20.2% |
//! | `--new-flow --rendering direct --range sdr` (Adobe RGB) 16.26 MP | 0.845 GB | 0.694 GB | +21.7% |
//! | `--new-flow --rendering direct --range sdr` (Adobe RGB) 18.66 MP | 0.950 GB | 0.795 GB | +19.5% |
//! | `measure-roll`, one 16.43 MP frame (explicit base) | 0.739 GB | 0.606 GB | +22.0% |
//! | `measure-roll`, one 18.66 MP frame (explicit base) | 0.821 GB | 0.685 GB | +19.7% |
//! | SDR TIFF 5.83 MP (explicit base) | 0.389 GB | 0.256 GB | +51.8% |
//! | linear float TIFF 5.83 MP (explicit base) | 0.349 GB | 0.221 GB | +57.8% |
//! | PQ TIFF 5.83 MP (explicit base) | 0.389 GB | 0.256 GB | +51.8% |
//! | PQ TIFF 18.66 MP (explicit base) | 0.950 GB | 0.795 GB | +19.4% |
//! | HLG TIFF 5.83 MP (explicit base) | 0.389 GB | 0.256 GB | +51.8% |
//! | HLG TIFF 18.66 MP (explicit base) | 0.950 GB | 0.795 GB | +19.4% |
//! | `--film-master` 5.83 MP (explicit base) | 0.349 GB | 0.221 GB | +57.8% |
//! | `--film-master` 18.66 MP (explicit base) | 0.821 GB | 0.683 GB | +20.2% |
//! | gain-map JPEG 5.83 MP (explicit base) | 0.543 GB | 0.362 GB | +50.0% |
//! | gain-map JPEG 16.51 MP (explicit base) | 1.293 GB | 1.016 GB | +27.2% |
//! | gain-map JPEG 18.66 MP (explicit base) | 1.443 GB | 1.153 GB | +25.1% |
//! | gain-map JPEG `--exposure 3` 18.66 MP (explicit base) | 1.443 GB | 1.158 GB | +24.7% |
//!
//! The two `measure-roll` rows (2026-09-23) were [`RunProfile::MeasureRoll`]'s first
//! calibration; its current one is in the set below.
//!
//! **The model is per frame, and so is the gate.** A multi-frame run peaks near its
//! largest frame only because [`crate::allocator`] returns each frame's big buffers
//! to the OS; `tests/multi_frame_memory.rs` holds it to that, and the before/after
//! numbers are in `docs/progress/io.md` (`multi-frame-memory-growth`). The rows above
//! predate the allocator, which lowered single-frame peaks too.
//!
//! Small frames run looser (+39.4% for the u16 18.66 MP run) because
//! [`ALLOWANCE_FIXED_BYTES`] stops being negligible — harmless, since they are nowhere
//! near any plausible budget. The `estimate --grid` and auto-interior rows measured
//! paths since retired (`film-base/holder-masked-measurement`); they stay as
//! calibration points for the rectangle sizes they gathered.
//!
//! The `--export-ir` rows measured a flag since retired; it measured no higher than the
//! same run without it.
//!
//! The five TIFF-HDR rows share one number per frame size because all three presets
//! peak at the **render** phase, which is identical across them (they share
//! `hdr::render_linear`); the u16 quantize buffer that distinguishes `hdr-*-tiff`
//! lands in the cheaper encode phase and never sets the peak. Their margin is wider
//! than `hdr-pq`'s at the same size because that AVIF preset's peak was at encode,
//! where its codec staging was fitted; here the peak is a phase built only from
//! enumerated buffers, so the residual is unmodelled allocator and writer overhead.
//! Every one of these estimates stays **above** measured, which is the direction the
//! gate requires.
//!
//! The two SDR rows are that same render peak — which is why the 74.65 MP estimate
//! is again 3.911 GB — but they are **measured**, not inherited from the structural
//! argument: 0.850 GB and 3.594 GB against 0.921 GB and 3.911 GB (1.08x / 1.09x), with
//! the enumerated buffers alone at 0.80x / 0.91x of measured, so the allowance is
//! covering real unmodelled overhead rather than padding. `display-p3` and
//! `compatibility` share one profile — same buffers, different destination gamut —
//! so the pair of frame sizes covers both.
//!
//! The four `--new-flow` rows (2026-09-22, `nf-core/minimal-end-to-end`, run while the
//! chain still sat behind that flag — today's default `convert`) are what
//! [`RunProfile::U16Tiff`]'s arithmetic rests on: a u16 `convert` on the removed chain
//! of the same two frames measured within 0.1 MB of each, and the pair solves to
//! ~42 B/px with ~10 MB fixed against the 38 B/px the enumerated buffers account —
//! `accounted` 0.89–0.94x of measured, the allowance covering the rest. The largest
//! scan was not on hand for this pair, so the rows are 14.45 and 18.66 MP.
//!
//! The four `--rendering direct` rows (2026-09-27, `nf-destinations/direct-preset`)
//! calibrate [`RunProfile::F32Tiff`] for the linear HDR destination — `direct`'s
//! default — with `accounted` 0.87x of measured at both sizes, and confirm the Adobe RGB
//! SDR TIFF shares [`RunProfile::U16Tiff`]: it measured within 33 KB of a Display
//! P3 run of the same frame at each size, as a matrix change inside an in-place map
//! should. Likewise, on a 16.6 MP frame (2026-09-29,
//! `nf-destinations/easy-destination-rows`), the float TIFF in each of its four gamuts,
//! the sRGB SDR TIFF and the sRGB-based gain map each peaked within 1 MB of the row
//! sharing their profile.
//!
//! The last twelve rows (2026-10-01, `nf-destinations/memory-profiles`) come from every
//! destination measured on four HDRi frames, 5.83 to 18.66 MP (no larger scan was on
//! hand); the 7.11 and 16.51 MP peaks, in `docs/progress/nf-destinations.md`, lie within
//! 6 MB of the lines fitted through the table's two sizes (the SDR and float TIFFs'
//! 18.66 MP points are their PQ and film-master twins'). The PQ and HLG TIFFs peaked
//! within 0.25 MB of the SDR TIFF, and the film master within 0.25 MB of the linear
//! float TIFF — run-to-run noise — so each shares its arm by measurement: 42.0 B/px +
//! 11 MB for [`RunProfile::U16Tiff`], 36.0 B/px + 11 MB for [`RunProfile::F32Tiff`],
//! against 38 and 32 B/px accounted. The gain map measured 61.7 B/px + 3 MB against 61
//! accounted; pushing the 16.51 and 18.66 MP frames +3 or +5 EV moved its peak by at
//! most 10 MB.
//!
//! The `hdr-pq` and `hdr-hlg` rows are the removed AVIF destination's: the `hdr-pq`
//! pair fitted its codec staging, and `docs/design/avif-removal.md` keeps that fit
//! for anyone restoring it.
//!
//! **Since `nf-core/release-decoded-image` (2026-10-08) every conversion row above
//! over-states its run**: each held the scan to the frame's end, 16 B/px more for HDRi
//! than the profiles now count. They stay as the evidence behind the constants they
//! fitted. The conversion profiles now rest on this set, explicit base, each cell
//! **Linux / macOS**: Linux x86_64 (`wait4`) on synthetic HDRi frames of 5.83 / 18.66 /
//! 74.65 MP; macOS/aarch64 (`/usr/bin/time -l`, the highest of 2–6 runs) on real HDRi
//! scans of 5.83 and 18.66 MP and a 74.65 MP frame tiled 2x2 from the 18.66 MP one. The
//! margin is over the higher peak.
//!
//! | destination | 5.83 MP | 18.66 MP | 74.65 MP | 74.65 MP model | margin |
//! |---|---|---|---|---|---|
//! | SDR TIFF | — / 0.116 GB | — / 0.347 GB | — / 1.355 GB | 1.679 GB | +23.9% |
//! | linear float TIFF | 0.112 / 0.115 GB | 0.343 / 0.347 GB | 1.350 / 1.354 GB | 1.679 GB | +24.0% |
//! | PQ TIFF | 0.112 / 0.117 GB | 0.343 / 0.347 GB | 1.350 / 1.355 GB | 1.679 GB | +23.9% |
//! | `--film-master` | 0.111 / 0.115 GB | 0.342 / 0.346 GB | 1.350 / 1.354 GB | 1.679 GB | +24.0% |
//! | gain-map JPEG | 0.221 / 0.221 GB | 0.693 / 0.697 GB | 2.750 / 2.747 GB | 3.654 GB | +32.9% |
//! | `measure-roll`, one frame | 0.112 / 0.115 GB | 0.343 / 0.346 GB | 1.351 / 1.354 GB | 1.679 GB | +24.0% |
//!
//! At `nf-core/release-decoded-image` each fell by the scan's 16 B/px against the
//! previous build on the same frame (the float TIFFs by 14, their peak now the decode
//! phase). `accounted` is 0.91–1.00x of
//! measured for the SDR and float TIFFs and `measure-roll`; the gain map peaks at 0.90x
//! of accounted, over-counted. macOS runs a constant 3–5 MB above Linux at every size.
//! Its 74.65 MP gain-map peak sometimes comes in lower (2.29–2.49 GB): the table keeps
//! the highest. The SDR TIFF and gain-map macOS cells are this branch's
//! (`nf-core/buffer-strategy`, below).
//!
//! **`nf-core/buffer-strategy` (2026-10-09) dropped the IR plane at the fixed decode.**
//! The SDR TIFF then peaked with the float ones (1.654 → 1.355 GB at 74.65 MP on macOS,
//! highest of five; its Linux cells were not re-measured), and every other destination
//! was unchanged: the float TIFFs and `measure-roll` already peaked at the decode, and
//! the gain map already dropped the plane before its split. Its model went down 4 B/px
//! with nothing measured moving.
//!
//! Peak RSS varies by a few tens of KB between identical runs; the frozen literals
//! in the tests are single observations, which is why the assertions are
//! `estimate >= measured` rather than equality.
//!
//! [`ALLOWANCE_PERCENT`] was set above the worst per-pixel overhead measured on macOS
//! against the 32 B/px base the float TIFF had then, **13.3%** (36.0–36.25 vs 32 B/px).
//! Today's base runs 1–3% over on both platforms from 18.66 MP up; the ~4 B/px
//! macOS once ran above Linux went with the scan's buffer. A small frame runs further
//! over its accounted buffers (10% on the 5.83 MP film master on macOS);
//! [`ALLOWANCE_FIXED_BYTES`] carries that.
//!
//! For the pre-fix three-image render peak (3.808 GB on the same 74.65 MP frame)
//! and the rest of the before/after set, see `docs/progress/io.md`
//! (`## memory-preflight`).
//!
//! ## Determinism (scoped — read before touching this)
//!
//! The *image output* is untouched: nothing here perturbs a pixel, and the
//! estimate is a pure function of the input shape plus the run's parameters. The
//! *pass/fail decision* is likewise machine-independent, because the default
//! budget is a **fixed constant** ([`DEFAULT_MAX_MEMORY_BYTES`]) rather than a
//! fraction of detected RAM. The one environment-dependent piece is the **warning
//! tier**, which compares the estimate against detected physical RAM
//! ([`detect_total_ram`]): the same input can warn on a small machine and stay
//! quiet on a large one — and on `convert`/`roll`/`measure-base`, where `--strict`
//! promotes warnings, can therefore *exit* differently. (`inspect` has no
//! `--strict`, so there the warning is report-only.) That is deliberate — a small
//! machine deserves the warning — but it means "same input + params ⇒ same exit"
//! holds only up to `--strict` plus the warn tier.

use crate::io::decode::ImageShape;
use crate::types::{NcError, OutDepth, Result};

use serde::Serialize;

/// Bytes per sample of the `f32` working representation every pixel buffer in the
/// pipeline uses (design-spec §4: 32-bit float linear working space throughout).
const F32_BYTES: u64 = 4;

/// Channels every *working* buffer carries, independent of the input file's
/// channel count: [`crate::types::LinearImage`]'s `rgb` is `w*h*3` by invariant
/// (enforced by its constructor) and `io::encode` writes RGB unconditionally. Only
/// the source-depth read buffer is sized from the file's own channel count.
const WORKING_CHANNELS: u64 = 3;

/// Default memory budget when `--max-memory` is not given: 6 GiB.
///
/// Deliberately a **fixed** constant, not a fraction of detected RAM, so the
/// pass/fail decision is machine-independent (see the determinism note above) —
/// the same scan either fits the budget everywhere or nowhere.
///
/// Sized against the worst *real* workload: a `convert` on the largest scan on hand
/// (74.65 MP HDRi) with a full-frame `--base-region` — the measure-once-reuse-`Dmin`
/// workflow design-spec §8 recommends — which accounts 30 B/px = 2.24 GB and
/// estimates **2.71 GB** with the allowance. 6 GiB admits it, and scans well beyond
/// it, while still catching the multi-GiB runaway the old 4 GiB *input* limit
/// permitted unchecked. A machine that wants a tighter or looser ceiling sets
/// `--max-memory`; the rejection message says so, and the RAM-aware warn tier is what
/// protects a small machine from a budget this size.
pub const DEFAULT_MAX_MEMORY_BYTES: u64 = 6 * 1024 * 1024 * 1024;

/// Proportional part of the allowance added to the accounted buffers, in percent.
/// Set above the worst per-pixel overhead measured on macOS, **13.3%**, against a
/// larger base than today's (the module doc's calibration section). Above the
/// overhead, not at it, so the estimate keeps a real margin rather than tracking one
/// machine's allocator exactly: the gate must err toward rejecting, never toward an
/// OOM. A small frame's larger relative overhead falls to [`ALLOWANCE_FIXED_BYTES`].
const ALLOWANCE_PERCENT: u64 = 15;

/// Fixed part of the allowance: the binary, static data, lcms2 profiles and
/// transforms, and the `tiff` writer's buffers — costs that don't scale with the
/// frame. 128 MiB.
///
/// Unconditional, so it is also a **floor** on every estimate: no input, however
/// small, can be admitted under a budget of 128 MiB or less. The rejection message
/// says so when the accounted buffers are the smaller part (a smaller frame cannot
/// help there).
const ALLOWANCE_FIXED_BYTES: u64 = 128 * 1024 * 1024;

/// Fraction of detected physical RAM (percent) above which an *within-budget*
/// estimate still warns. A run that wants most of the machine's memory will fight
/// the page cache and swap even though it is nominally allowed.
const RAM_WARN_PERCENT: u64 = 70;

/// Which pipeline a preflight is sizing. The commands allocate very differently —
/// `inspect`/`measure-base` stop after decode, so gating them on the full-pipeline
/// peak would reject inputs they could handle fine.
///
/// One variant per **shape of buffers** a destination holds, not one per destination.
/// A destination that holds a new shape gets its own variant, calibrated before it
/// ships: measure peak RSS on **two** frame sizes and solve for the per-pixel slope
/// and the fixed cost (one size cannot separate them), and leave the enumerated
/// `accounted` bytes slightly *under* measured — [`ALLOWANCE_PERCENT`] exists to cover
/// allocator overhead, so padding the buffers as well double-counts it and rejects
/// runs that fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunProfile {
    /// `convert` / `roll` into a **16-bit TIFF**: the fixed decode, the chain (one
    /// branch, `chain::render`), and an SDR destination or a coded HDR one (PQ/HLG
    /// codes; `hdr::from_new_chain` and `hdr::encode_transfer` both work in place).
    ///
    /// It holds the decoded image's RGB, which the fixed decode rewrites in place and
    /// the chain moves through every boundary and transforms in place, then a 3x2 B
    /// quantize buffer with `tiff` streaming strips. With nothing sampled, its **decode
    /// and encode** phases tie at 18 B/px.
    /// Measured for the SDR TIFF and the PQ TIFF (the module doc's calibration table).
    ///
    /// **One branch only.** A gain-map pair (`chain::render_pair`) copies the graded
    /// image and holds two working buffers through fit range and fit gamut, so it has
    /// an arm of its own, [`GainMapJpeg`](Self::GainMapJpeg).
    U16Tiff,
    /// Into a **32-bit float TIFF**: the linear HDR destination, or the film master.
    /// [`U16Tiff`](Self::U16Tiff)'s buffers with no quantize buffer — f32 is written
    /// verbatim, so with nothing sampled its peak is the **decode** phase. Measured for
    /// the linear HDR TIFF and the film master (the module doc's calibration table).
    F32Tiff,
    /// Into the **gain-map JPEG**: `chain::render_pair` — the chain's buffer and the
    /// graded copy it splits off — then the full-resolution f32 gains. The HDR
    /// rendition and the gains are dropped as soon as the next buffer is built from
    /// them, but are summed, not competed (the module doc's retention rule). Encode
    /// adds the u8 base, the half-resolution map and both JPEGs, the JPEGs at a fitted
    /// size. Measured (the module doc's calibration table).
    GainMapJpeg,
    /// `inspect` / `measure-base`: decode, then sample — no render, no encode.
    DecodeOnly,
    /// `measure-roll`, per frame: the fixed decode into linear ACEScg, then a strided
    /// sample of it — no chain, no encode.
    ///
    /// Holds [`U16Tiff`](Self::U16Tiff)'s render-phase buffer — the decoded image,
    /// decoded and mapped into ACEScg in place — and nothing after it, so it peaks at
    /// the **decode** phase. The per-frame samples (~1.5 MB each,
    /// `roll_white::FRAME_SAMPLE_PIXELS`) are not in this model: they peak after the
    /// frame loop at three per frame (film RGB, its ACEScg copy, the pooled white's
    /// per-channel copy), ~160 MB on a 36-frame roll.
    MeasureRoll,
}

/// The rectangle a run's film-base sampling will gather into per-channel `f32`
/// vectors (`film_base::region_channels`, 12 B per sampled pixel) — a stated
/// `--base-region` — so the film-base phase can be sized before anything is decoded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SamplePlan {
    /// The sampled rectangle, in pixels. `0` for none.
    pub rect_pixels: u64,
}

impl SamplePlan {
    /// Nothing is gathered: an explicit `--film-base`, or the effective-area
    /// measurement, whose histogram is a fixed cost.
    pub fn none() -> Self {
        Self::default()
    }

    /// One stated rectangle of `pixels` px.
    pub fn rect(pixels: u64) -> Self {
        Self {
            rect_pixels: pixels,
        }
    }

    /// Pixels this plan gathers, for `shape`.
    ///
    /// **Clamped to the frame.** `rect_pixels` comes from a user-supplied
    /// `--base-region`, which this stage has no business validating:
    /// `film_base::region_channels` rejects an out-of-bounds one as a *usage* error
    /// (exit 2). Estimating the raw `w*h` instead would report a typo'd
    /// `--base-region 0,0,999999,999999` as a 12852 GiB **resource** rejection
    /// (exit 6), burying the real mistake behind the wrong exit code. No legal
    /// sample can exceed the frame, so clamping cannot under-estimate a real run.
    fn sampled_pixels(&self, shape: &ImageShape) -> u64 {
        self.rect_pixels
            .min(shape.width as u64 * shape.height as u64)
    }
}

/// The estimated peak allocation of a run, with the per-phase breakdown that
/// produced it. Serialized into the JSON report so the number the gate decided on
/// is auditable (and comparable against a measured peak RSS).
///
/// Which terms generalize beyond the shipped path: `decode_bytes`'s source read
/// buffer is the only term sized from the *file's* channel count and bit depth;
/// every other term is pinned to the working representation the pipeline actually
/// uses — 3-channel `f32` in, 3-channel `u16`/`f32` out ([`WORKING_CHANNELS`]) —
/// because that is an invariant of `LinearImage`/`io::encode`, not a property of
/// the input. A future non-3-channel input (a `Gray(16)` B&W scan) changes only
/// the read-buffer term.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PeakEstimate {
    /// What the gate compares against the budget: [`accounted_bytes`] plus the
    /// allowance for allocator slack and fixed costs.
    ///
    /// [`accounted_bytes`]: Self::accounted_bytes
    pub estimated_peak_bytes: u64,
    /// The peak of the named full-frame buffers alone — no allowance.
    pub accounted_bytes: u64,
    /// Named-buffer total while decoding (`f32` image + the transient source-depth
    /// read buffer).
    pub decode_bytes: u64,
    /// Named-buffer total during film-base estimation: the decoded image plus the
    /// three `f32` channel vectors of the largest sampled rectangle ([`SamplePlan`]).
    /// Equals the decoded image alone when nothing is sampled (an explicit
    /// `--film-base`).
    pub film_base_bytes: u64,
    /// Named-buffer total during the render (the positive, written over the decoded
    /// image). Zero for [`RunProfile::DecodeOnly`], which never renders.
    pub render_bytes: u64,
    /// Named-buffer total during encode (rendered + quantize buffers).
    /// Zero for [`RunProfile::DecodeOnly`], which never encodes.
    pub encode_bytes: u64,
}

/// Where the active budget came from — reported so a rejection is traceable to
/// the flag or to the built-in default. The wire form of [`Budget`]'s
/// discriminant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BudgetSource {
    /// [`DEFAULT_MAX_MEMORY_BYTES`].
    Default,
    /// An explicit `--max-memory`.
    Flag,
}

/// The resolved memory budget for a run.
///
/// One enum rather than a `{ bytes, source }` pair: the pair could represent a
/// `Default` budget with a non-default byte count (and the rejection message
/// branches on the source while the comparison uses the bytes, so the two could
/// disagree about what rejected the run). This also keeps
/// [`DEFAULT_MAX_MEMORY_BYTES`] in exactly one place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Budget {
    /// No `--max-memory`: the fixed [`DEFAULT_MAX_MEMORY_BYTES`].
    Default,
    /// An explicit `--max-memory` value, in bytes.
    Explicit(u64),
}

impl Budget {
    /// Resolve the budget from an optional `--max-memory` value.
    pub fn resolve(flag: Option<u64>) -> Self {
        match flag {
            Some(bytes) => Self::Explicit(bytes),
            None => Self::Default,
        }
    }

    /// The ceiling in bytes.
    pub fn bytes(self) -> u64 {
        match self {
            Self::Default => DEFAULT_MAX_MEMORY_BYTES,
            Self::Explicit(bytes) => bytes,
        }
    }

    /// The reported provenance of this budget.
    fn source(self) -> BudgetSource {
        match self {
            Self::Default => BudgetSource::Default,
            Self::Explicit(_) => BudgetSource::Flag,
        }
    }
}

/// The preflight's verdict for a run that is *allowed to proceed*. Over-budget
/// runs never produce a verdict — they fail with [`NcError::Resource`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Within budget, and not a large fraction of the machine's RAM.
    Ok,
    /// Within budget, but above [`RAM_WARN_PERCENT`] of detected physical RAM.
    Warn,
}

/// The preflight decision for the JSON report: the estimate, the budget it was
/// checked against, the verdict, and the detected RAM the warn tier used.
///
/// `budget_bytes`/`budget_source` are the two flat keys the documented report
/// contract carries; both are derived from the [`Budget`] enum at construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct MemoryReport {
    #[serde(flatten)]
    pub estimate: PeakEstimate,
    pub budget_bytes: u64,
    pub budget_source: BudgetSource,
    pub decision: Verdict,
    /// Detected physical RAM, when the platform could report it. Absent rather
    /// than guessed — the warn tier simply doesn't fire without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_total_ram_bytes: Option<u64>,
}

/// Run the preflight: size the run from its input shape, compare against the
/// budget, and either reject it loudly or return the report (with the warn-tier
/// verdict) for the orchestrator to record.
///
/// `total_ram` is injected rather than detected here so the whole decision stays
/// a pure function — the orchestrator passes [`detect_total_ram`].
///
/// **Must be called before the input is decoded.** Its whole purpose is to run
/// while nothing large is allocated yet; sizing a run from an already-decoded
/// image would OOM on exactly the inputs this gate exists to reject.
pub fn preflight(
    shape: &ImageShape,
    profile: RunProfile,
    sampling: SamplePlan,
    budget: Budget,
    total_ram: Option<u64>,
) -> Result<MemoryReport> {
    let estimate = estimate_peak(shape, profile, sampling)?;
    if estimate.estimated_peak_bytes > budget.bytes() {
        // When the frame's own buffers are the smaller part of the estimate, the
        // fixed allowance floor is what the budget can't fit — telling the user to
        // convert a smaller frame there would be impossible advice.
        let advice = if estimate.accounted_bytes < ALLOWANCE_FIXED_BYTES {
            format!(
                "raise it with --max-memory: every run costs a fixed {} on top of its \
                 image buffers (the binary, lcms2, the tiff writer), so no smaller \
                 frame fits this budget either",
                human_bytes(ALLOWANCE_FIXED_BYTES)
            )
        } else {
            "raise it with --max-memory or convert a smaller frame".to_string()
        };
        // Exact byte counts alongside the rounded forms: `human_bytes` keeps one
        // decimal, so a near-miss would otherwise print the self-contradictory
        // "estimated peak 1.0 GiB exceeds the default budget of 1.0 GiB" — and the
        // rejection path emits no JSON report, so this message is the only channel
        // an agent has for choosing a new `--max-memory`.
        return Err(NcError::Resource(format!(
            "estimated peak memory {} ({} bytes) exceeds the {} budget of {} ({} bytes) \
             ({}x{} px, {} channels, {}-bit{}); {advice}",
            human_bytes(estimate.estimated_peak_bytes),
            estimate.estimated_peak_bytes,
            match budget.source() {
                BudgetSource::Flag => "--max-memory",
                BudgetSource::Default => "default",
            },
            human_bytes(budget.bytes()),
            budget.bytes(),
            shape.width,
            shape.height,
            shape.channels,
            shape.bits_per_sample,
            if shape.ir_present { " + IR plane" } else { "" },
        )));
    }
    let decision = match total_ram {
        Some(ram) if estimate.estimated_peak_bytes > ram / 100 * RAM_WARN_PERCENT => Verdict::Warn,
        _ => Verdict::Ok,
    };
    Ok(MemoryReport {
        estimate,
        budget_bytes: budget.bytes(),
        budget_source: budget.source(),
        decision,
        detected_total_ram_bytes: total_ram,
    })
}

/// The `--strict`-promotable warning text for a [`Verdict::Warn`] report, or
/// `None` when there is nothing to warn about. Kept next to the model so the
/// threshold and its wording can't drift apart.
///
/// The `Warn` verdict is only reachable with a detected RAM figure, and the guard
/// lives *here* (rather than at the call site) so a second caller cannot forget it
/// and render "more than 70% of this machine's unknown of RAM".
pub fn warn_message(report: &MemoryReport) -> Option<String> {
    let (Verdict::Warn, Some(ram)) = (report.decision, report.detected_total_ram_bytes) else {
        return None;
    };
    Some(format!(
        "estimated peak memory {} is more than {RAM_WARN_PERCENT}% of this machine's {} of RAM; \
         the run may swap or be killed by the OS",
        human_bytes(report.estimate.estimated_peak_bytes),
        human_bytes(ram),
    ))
}

/// Size a run's peak allocation from its input shape — the single source of truth
/// for the preflight, the report, and (later) `io/streaming-tiled-io`'s go/no-go.
/// Pure; see the module doc for the model and its calibration.
///
/// Arithmetic is checked throughout: a corrupt header advertising absurd
/// dimensions must surface as a loud error here (before anything is allocated),
/// never wrap into a small, permissive estimate.
pub fn estimate_peak(
    shape: &ImageShape,
    profile: RunProfile,
    sampling: SamplePlan,
) -> Result<PeakEstimate> {
    let overflow = || {
        NcError::Other(format!(
            "image dimensions {}x{} overflow the memory sizing model",
            shape.width, shape.height
        ))
    };
    let mul = |a: u64, b: u64| a.checked_mul(b).ok_or_else(overflow);
    let sum = |a: u64, b: u64| a.checked_add(b).ok_or_else(overflow);

    let pixels = mul(shape.width as u64, shape.height as u64)?;
    let src_sample = shape.bits_per_sample.div_ceil(8) as u64;

    // One interleaved f32 RGB working buffer (always 3-channel), and the
    // source-depth read buffer it is built from — the one term that follows the
    // *file's* channel count (both live at once inside
    // `io::decode::read_plane_u16`).
    let rgb32 = mul(pixels, mul(WORKING_CHANNELS, F32_BYTES)?)?;
    let rgb_src = mul(pixels, mul(shape.channels as u64, src_sample)?)?;
    // The IR plane, same two forms (single-channel). `ir_src` uses IFD0's bit
    // depth, not the IR page's own — harmless only because `decode` requires both
    // pages to be 16-bit (and rejects the file otherwise).
    let (ir32, ir_src) = if shape.ir_present {
        (mul(pixels, F32_BYTES)?, mul(pixels, src_sample)?)
    } else {
        (0, 0)
    };

    // One fully-decoded image resident in f32: RGB + the IR plane, which lives until
    // the fixed decode drops it.
    let image = sum(rgb32, ir32)?;

    // Decode, in two moments: reading RGB (f32 RGB buffer + the u16 read buffer it
    // is built from — the IR plane does not exist yet), then reading IR (f32 RGB
    // buffer + the IR read buffer + the f32 IR plane built from it). The RGB read
    // buffer is freed before the IR one is allocated, so the peak is the larger
    // moment — with `rgb32` (not the whole decoded image) as the common base.
    let decode_bytes = sum(rgb32, rgb_src.max(sum(ir_src, ir32)?))?;

    // The three f32 channel vectors `film_base::region_channels` materializes for
    // the largest rectangle this run samples. Rectangles are gathered one at a
    // time, so the largest sets the term (measured: sampling two full-frame
    // rectangles peaks identically to sampling one).
    let sampled = mul(
        sampling.sampled_pixels(shape),
        mul(WORKING_CHANNELS, F32_BYTES)?,
    )?;

    // Film base (stage 2): the decoded image plus that sample.
    let film_base_bytes = sum(image, sampled)?;

    // From the render on, the chain's buffer *is* the decoded image's RGB: the fixed
    // decode rewrites it in place and drops the IR plane, and every stage moves it on.
    // `sampled` is added to each later phase, not competed against them: the module
    // doc's retention rule. Measured: treating a full-frame `--base-region` sample as a
    // *competing* phase under-estimated that run by 10%.
    let render = sum(rgb32, sampled)?;
    // Encode for the TIFF profiles: the u16 staging buffer (none at f32 depth, which
    // writes the working buffer verbatim). The output is RGB whatever the input was,
    // so it follows `WORKING_CHANNELS`.
    let tiff_encode = |depth: OutDepth| -> Result<u64> {
        let quantize = match depth {
            OutDepth::U16 => mul(pixels, mul(WORKING_CHANNELS, 2)?)?,
            OutDepth::F32 => 0,
        };
        sum(render, quantize)
    };
    let (render_bytes, encode_bytes) = match profile {
        RunProfile::DecodeOnly => (0, 0),
        RunProfile::MeasureRoll => (render, 0),
        RunProfile::U16Tiff => (render, tiff_encode(OutDepth::U16)?),
        RunProfile::F32Tiff => (render, tiff_encode(OutDepth::F32)?),
        RunProfile::GainMapJpeg => {
            // Render: the chain's buffer + the split copy + the f32 gains.
            let render = mul(rgb32, 3)?;
            // Encode: all of that retained, plus the u8 base (3 B/px), the u8 map
            // (0.75), the gain-map JPEG, and the base JPEG that `assemble` grows in
            // place. The base JPEG's doubling growth and
            // that final copy make it up to ~3x its length under the retention rule.
            // The 5 B/px is a content assumption fitted to measured JPEGs (at most
            // 0.55 B/px: a thin grainy frame pushed +5 EV), not an enumeration; past it,
            // the margin is the allowance (~7.9 B/px at 74.65 MP: 15% of 41 B/px and
            // the fixed part).
            let byte_staging = mul(pixels, 5)?;
            (
                sum(render, sampled)?,
                sum(sum(render, byte_staging)?, sampled)?,
            )
        }
    };

    let accounted_bytes = decode_bytes
        .max(film_base_bytes)
        .max(render_bytes)
        .max(encode_bytes);
    let allowance = sum(
        accounted_bytes / 100 * ALLOWANCE_PERCENT,
        ALLOWANCE_FIXED_BYTES,
    )?;
    Ok(PeakEstimate {
        estimated_peak_bytes: sum(accounted_bytes, allowance)?,
        accounted_bytes,
        decode_bytes,
        film_base_bytes,
        render_bytes,
        encode_bytes,
    })
}

/// Physical RAM in bytes, when the platform can report it cheaply — used only by
/// the warn tier, never by the (deliberately fixed) default budget.
///
/// Fail-soft by design: an unsupported platform, a missing `/proc`, a failed
/// syscall, or an unparsable value returns `None`, which disables the warning
/// rather than failing a run that is within budget.
pub fn detect_total_ram() -> Option<u64> {
    // Bound to a `let` per platform rather than left as three cfg'd tail
    // expressions: with tails, adding any statement below would turn the surviving
    // block into a `#[must_use]` `Option` statement — an `unused_must_use` error
    // under `-D warnings` that could go red on one target while staying green on
    // the other. This shape cannot spring that trap.
    #[cfg(target_os = "linux")]
    let detected = linux_total_ram();
    #[cfg(target_os = "macos")]
    let detected = macos_total_ram();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let detected = None;
    detected
}

/// Installed RAM from the `hw.memsize` sysctl — the documented source on Darwin,
/// which has no `/proc` equivalent. Read-only, allocation-free, fail-soft.
#[cfg(target_os = "macos")]
fn macos_total_ram() -> Option<u64> {
    let mut size: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: `sysctlbyname` writes at most `len` bytes through `oldp` — a `u64`
    // this frame owns, correctly aligned and valid for the whole call — and reads
    // `name` as a NUL-terminated string (a `c"…"` literal, so NUL-terminated by
    // construction and `'static`). `newp`/`newlen` are the documented null/0 for a
    // read. Nothing is retained past the call and no unwinding crosses it.
    //
    // Note what this does NOT assume: `sysctl(3)` may copy out a partial value and
    // still return -1 (ENOMEM), so the result is trusted only when `rc == 0` *and*
    // the kernel reports having written exactly the 8 bytes a `uint64_t` needs.
    // `size` is pre-zeroed, so even a short copyout is a wrong value, never UB.
    let rc = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut size).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && len == std::mem::size_of::<u64>() && size > 0).then_some(size)
}

/// Total RAM available to this process on Linux: `/proc/meminfo`'s `MemTotal`
/// capped by any cgroup memory limit.
///
/// `MemTotal` reports the **host's** RAM and ignores cgroup limits, so inside a
/// container capped well below the host the warn tier would compare against a
/// number the process can never reach — exactly where an OOM kill is likeliest.
/// Read the cgroup ceiling too (v2 `memory.max`, else v1
/// `memory/memory.limit_in_bytes`) and take the lower of the two. Fail-soft
/// throughout: any missing file or unparsable value degrades to the other source,
/// or to `None`, never to an error.
///
/// Deliberately a plain function rather than inline `#[cfg]` code so it is
/// type-checked on every target (the parse helpers are unit-tested everywhere too);
/// only Linux ever calls it.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_total_ram() -> Option<u64> {
    let mem_total = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .as_deref()
        .and_then(parse_meminfo_total);
    let cgroup = cgroup_limit_paths()
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .find_map(|text| parse_cgroup_limit(&text));
    match (mem_total, cgroup) {
        (Some(total), Some(limit)) => Some(total.min(limit)),
        (total, None) => total,
        (None, limit) => limit,
    }
}

/// Candidate cgroup memory-limit files, most specific first.
///
/// The **process's own** cgroup is what bounds it, and under Kubernetes or a
/// systemd service that cgroup is nested — the limit lives at the path named in
/// `/proc/self/cgroup`, not at the root. Reading only the root files would report
/// `max` (or a permissive ancestor limit) in exactly the containerized setups the
/// cgroup check exists for, leaving the warn tier silent where OOM is likeliest.
/// So: derive the process's own v2 and v1 paths first, then walk *up* to the root
/// as a fallback, since a nested cgroup may itself declare no limit while an
/// ancestor does.
///
/// Not `cfg`-gated (like the parsers beside it): only its *caller* is Linux-only,
/// so the path logic still compiles and unit-tests on every target — this machine
/// can build only `aarch64-apple-darwin`, and Linux-only code that nothing here
/// compiles is code that first fails in CI.
fn cgroup_limit_paths() -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(text) = std::fs::read_to_string("/proc/self/cgroup") {
        let (v2, v1) = parse_self_cgroup(&text);
        // v2: a single unified hierarchy line, `0::<path>`.
        if let Some(rel) = v2 {
            for anc in ancestors_of(&rel) {
                paths.push(format!("/sys/fs/cgroup{anc}/memory.max"));
            }
        }
        // v1: the `memory` controller's own line.
        if let Some(rel) = v1 {
            for anc in ancestors_of(&rel) {
                paths.push(format!("/sys/fs/cgroup/memory{anc}/memory.limit_in_bytes"));
            }
        }
    }
    // Root fallbacks, for a non-nested cgroup or an unreadable /proc/self/cgroup.
    paths.push("/sys/fs/cgroup/memory.max".to_string());
    paths.push("/sys/fs/cgroup/memory/memory.limit_in_bytes".to_string());
    paths
}

/// The v2 and v1-`memory` relative cgroup paths from a `/proc/self/cgroup` body.
/// Lines are `hierarchy-ID:controller-list:path`; v2 is the `0::` line, and v1's
/// memory controller is whichever line lists `memory` among its controllers. A
/// root path (`/`) yields `None` — there is nothing nested to resolve.
fn parse_self_cgroup(text: &str) -> (Option<String>, Option<String>) {
    let (mut v2, mut v1) = (None, None);
    for line in text.lines() {
        let mut parts = line.splitn(3, ':');
        let (Some(id), Some(controllers), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if path == "/" || path.is_empty() {
            continue;
        }
        if id == "0" && controllers.is_empty() {
            v2 = Some(path.to_string());
        } else if controllers.split(',').any(|c| c == "memory") {
            v1 = Some(path.to_string());
        }
    }
    (v2, v1)
}

/// A cgroup path and each of its ancestors, most specific first, ending at the
/// root (`""`). `/kubepods/podXYZ` yields `["/kubepods/podXYZ", "/kubepods", ""]`.
fn ancestors_of(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = path.trim_end_matches('/');
    while !cur.is_empty() {
        out.push(cur.to_string());
        cur = match cur.rfind('/') {
            Some(0) | None => "",
            Some(i) => &cur[..i],
        };
    }
    out.push(String::new());
    out
}

/// Bytes of `MemTotal` (reported in kB) from a `/proc/meminfo` body.
fn parse_meminfo_total(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    kb.checked_mul(1024)
}

/// Bytes of a cgroup memory-limit file body, or `None` when it declares no limit:
/// cgroup v2 writes the literal `max`, and v1 writes an enormous sentinel
/// (`u64::MAX` rounded down to a page multiple) — treating either as a real
/// ceiling would make the warn tier nonsense.
fn parse_cgroup_limit(text: &str) -> Option<u64> {
    let t = text.trim();
    if t == "max" {
        return None;
    }
    let bytes: u64 = t.parse().ok()?;
    // Anything at exbibyte scale is the "unlimited" sentinel, not a limit.
    (bytes > 0 && bytes < 1 << 60).then_some(bytes)
}

/// Format a byte count for a human-facing message: GiB/MiB/KiB with one decimal.
/// Report fields carry the exact byte counts, so this is presentation only.
fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= KIB * KIB * KIB {
        format!("{:.1} GiB", b / (KIB * KIB * KIB))
    } else if b >= KIB * KIB {
        format!("{:.1} MiB", b / (KIB * KIB))
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// Parse a `--max-memory` value: a byte count with an optional unit suffix
/// (`4GiB`, `4G`, `4096MB`, `512M`, `2000000000`). Binary and decimal suffixes are
/// both accepted and mean what they say (`GiB` = 1024³, `GB` = 1000³).
///
/// Rejects zero and unparsable values loudly as usage errors (exit 2) — a
/// silently-ignored budget flag would leave the run unguarded, and a zero budget
/// can never admit any conversion.
pub fn parse_max_memory(s: &str) -> Result<u64> {
    let t = s.trim();
    let digits = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let (number, unit) = t.split_at(digits);
    let value: u64 = number.parse().map_err(|_| {
        NcError::Usage(format!(
            "invalid --max-memory {s:?}: expected a byte count with an optional \
             unit (e.g. 4GiB, 512MB, 2000000000)"
        ))
    })?;
    let multiplier: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kib" => 1024,
        "kb" => 1000,
        "m" | "mib" => 1024 * 1024,
        "mb" => 1000 * 1000,
        "g" | "gib" => 1024 * 1024 * 1024,
        "gb" => 1000 * 1000 * 1000,
        "t" | "tib" => 1024_u64.pow(4),
        "tb" => 1000_u64.pow(4),
        other => {
            return Err(NcError::Usage(format!(
                "invalid --max-memory unit {other:?} in {s:?}: expected one of \
                 B, KiB/KB, MiB/MB, GiB/GB, TiB/TB"
            )));
        }
    };
    let bytes = value.checked_mul(multiplier).ok_or_else(|| {
        NcError::Usage(format!("--max-memory {s:?} overflows a 64-bit byte count"))
    })?;
    if bytes == 0 {
        return Err(NcError::Usage(
            "--max-memory must be greater than zero".into(),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped input shape: full-resolution 16-bit RGB, optional IR plane.
    fn shape(width: u32, height: u32, ir: bool) -> ImageShape {
        ImageShape::new(width, height, 3, 16, ir).expect("valid shape")
    }

    fn convert_u16() -> RunProfile {
        RunProfile::U16Tiff
    }

    #[test]
    fn which_phase_peaks_is_per_profile_and_measured_not_assumed() {
        // Pins the actual peak phase of every profile, because a plain-language
        // claim about it was wrong twice on the removed chain's profiles. No category,
        // then. Just the truth, so the next author reads it off a test instead of a
        // sentence.
        // Every phase that reaches the peak is named, so a tie is pinned as one.
        let peak_phases = |profile, ir| {
            let e = estimate_peak(&shape(10, 10, ir), profile, SamplePlan::none()).unwrap();
            [
                ("decode", e.decode_bytes),
                ("render", e.render_bytes),
                ("encode", e.encode_bytes),
            ]
            .into_iter()
            .filter(|&(_, bytes)| bytes == e.accounted_bytes)
            .map(|(phase, _)| phase)
            .collect::<Vec<_>>()
            .join("+")
        };
        // (profile, HDRi, RGB-only)
        for (profile, with_ir, without_ir) in [
            // The decode's 6 B/px read buffers (u16 RGB, or u16 + f32 IR) equal the
            // quantize buffer.
            (RunProfile::U16Tiff, "decode+encode", "decode+encode"),
            // The chain holds one image, so the decode's read buffers outweigh it, and
            // f32 is written verbatim.
            (RunProfile::F32Tiff, "decode", "decode"),
            // Encode retains every render buffer and adds the byte staging.
            (RunProfile::GainMapJpeg, "encode", "encode"),
            (RunProfile::MeasureRoll, "decode", "decode"),
        ] {
            assert_eq!(peak_phases(profile, true), with_ir, "{profile:?} HDRi");
            assert_eq!(
                peak_phases(profile, false),
                without_ir,
                "{profile:?} RGB-only"
            );
        }
    }

    /// The largest scan on hand (`largest.tif`), the calibration reference.
    fn big() -> ImageShape {
        shape(10368, 7200, true)
    }

    #[test]
    fn phase_totals_match_the_documented_bytes_per_pixel() {
        // The model, pinned per phase against the hand-derived per-pixel costs in
        // the module doc (HDRi: 18 / 12 / 18 B/px, film-base 16 + 12·s). A change
        // here is a change to what the gate promises, so it must be deliberate.
        let px = 1000u64 * 1000;
        let e = estimate_peak(&shape(1000, 1000, true), convert_u16(), SamplePlan::none()).unwrap();
        assert_eq!(e.decode_bytes, 18 * px);
        // Nothing sampled (explicit base): the decoded image alone.
        assert_eq!(e.film_base_bytes, 16 * px);
        // The decode dropped the IR plane.
        assert_eq!(e.render_bytes, 12 * px);
        assert_eq!(e.encode_bytes, 18 * px);
        assert_eq!(e.accounted_bytes, 18 * px);

        // Without an IR plane only the film-base phase changes: 18 / 12 / 12 / 18.
        let e =
            estimate_peak(&shape(1000, 1000, false), convert_u16(), SamplePlan::none()).unwrap();
        assert_eq!(e.decode_bytes, 18 * px);
        assert_eq!(e.film_base_bytes, 12 * px);
        assert_eq!(e.render_bytes, 12 * px);
        assert_eq!(e.encode_bytes, 18 * px);
    }

    #[test]
    fn film_base_phase_counts_the_sampled_rectangle() {
        // The film-base phase is the decoded image plus 12 B per sampled pixel —
        // the omission that let `inspect`/`measure-base` exceed their own estimate.
        let px = 1000u64 * 1000;
        let s = shape(1000, 1000, true);

        // A full-frame rectangle (`--base-region 0,0,w,h`) is the worst case: 28 B/px.
        let whole = estimate_peak(&s, RunProfile::DecodeOnly, SamplePlan::rect(px)).unwrap();
        assert_eq!(whole.film_base_bytes, 28 * px);
        // …and it is the peak for a decode-only run (above decode's 18 B/px).
        assert_eq!(whole.accounted_bytes, whole.film_base_bytes);

        // …and it does NOT leave `convert` alone. The sample is freed before the
        // render, but the retention rule keeps it in the later phases: render
        // 12 + 12, encode 18 + 12 = 30 B/px. Pinned because the
        // first version of this model let the phase merely *compete* with encode
        // and under-estimated a real full-frame-region convert by 10.2%.
        let convert = estimate_peak(&s, convert_u16(), SamplePlan::rect(px)).unwrap();
        assert_eq!(convert.render_bytes, 24 * px);
        assert_eq!(convert.encode_bytes, 30 * px);
        assert_eq!(convert.accounted_bytes, 30 * px);
        // With nothing sampled, the retained term is zero and encode is 18 again.
        let explicit = estimate_peak(&s, convert_u16(), SamplePlan::none()).unwrap();
        assert_eq!(explicit.encode_bytes, 18 * px);

        // Nothing sampled (an explicit base, or the effective-area histogram): the
        // phase is the decoded image alone.
        let none = estimate_peak(&s, RunProfile::DecodeOnly, SamplePlan::none()).unwrap();
        assert_eq!(none.film_base_bytes, 16 * px);

        // The later phases no longer hold the IR plane, so on an HDRi scan the film-base
        // phase is a float TIFF's and `measure-roll`'s peak once the sample passes a
        // sixth of the frame (16 + 12·s against the decode's 18); below that, the decode.
        for profile in [RunProfile::F32Tiff, RunProfile::MeasureRoll] {
            let e = estimate_peak(&s, profile, SamplePlan::rect(px)).unwrap();
            assert_eq!(e.accounted_bytes, e.film_base_bytes, "{profile:?}");
            assert_eq!(e.film_base_bytes, 28 * px, "{profile:?}");
            let tenth = estimate_peak(&s, profile, SamplePlan::rect(px / 10)).unwrap();
            assert_eq!(tenth.accounted_bytes, tenth.decode_bytes, "{profile:?}");
        }
    }

    #[test]
    fn f32_output_skips_the_quantize_buffer() {
        let px = 1000u64 * 1000;
        // f32 writes the working buffer verbatim — encode allocates nothing extra,
        // so encode is the one image (12 B/px) and the decode (18 B/px) is the peak.
        let f32_out = estimate_peak(
            &shape(1000, 1000, true),
            RunProfile::F32Tiff,
            SamplePlan::none(),
        )
        .unwrap();
        assert_eq!(f32_out.encode_bytes, 12 * px);
        assert_eq!(f32_out.accounted_bytes, 18 * px);

        // u16 stages a 6 B/px RGB buffer on top.
        let u16_out = estimate_peak(
            &shape(1000, 1000, true),
            RunProfile::U16Tiff,
            SamplePlan::none(),
        )
        .unwrap();
        assert_eq!(u16_out.encode_bytes, 18 * px);
    }

    #[test]
    fn working_buffers_are_sized_from_the_working_channel_count() {
        // Only the source read buffer follows the file's channel count; every
        // working buffer is 3-channel by `LinearImage`/`io::encode` invariant. A
        // hypothetical 1-channel 16-bit input (`decode` rejects it today) must not
        // shrink the f32/u16 buffers to a third.
        let px = 1000u64 * 1000;
        let gray = ImageShape::new(1000, 1000, 1, 16, false).unwrap();
        let e = estimate_peak(&gray, convert_u16(), SamplePlan::none()).unwrap();
        // rgb32 (12) + the 1-channel u16 read buffer (2).
        assert_eq!(e.decode_bytes, 14 * px);
        // One 3-channel f32 image + the 3-channel u16 quantize buffer.
        assert_eq!(e.render_bytes, 12 * px);
        assert_eq!(e.encode_bytes, 18 * px);
    }

    #[test]
    fn decode_only_profile_counts_no_render_or_encode() {
        // `inspect`/`measure-base` must not be gated on a render they never run. With a
        // sample, where a conversion's later phases retain it and outweigh the film-base
        // phase (with none, a u16 conversion's peak ties the decode's).
        let (s, sample) = (shape(1000, 1000, true), SamplePlan::rect(1000 * 1000));
        let e = estimate_peak(&s, RunProfile::DecodeOnly, sample).unwrap();
        assert_eq!(e.render_bytes, 0);
        assert_eq!(e.encode_bytes, 0);
        assert!(
            e.accounted_bytes
                < estimate_peak(&s, convert_u16(), sample)
                    .unwrap()
                    .accounted_bytes
        );
    }

    #[test]
    fn allowance_is_applied_on_top_of_the_accounted_buffers() {
        let e = estimate_peak(&shape(1000, 1000, true), convert_u16(), SamplePlan::none()).unwrap();
        let expected =
            e.accounted_bytes + e.accounted_bytes / 100 * ALLOWANCE_PERCENT + ALLOWANCE_FIXED_BYTES;
        assert_eq!(e.estimated_peak_bytes, expected);
        assert!(e.estimated_peak_bytes > e.accounted_bytes);
    }

    #[test]
    fn estimate_pins_the_model_output_for_the_calibration_shapes() {
        // Regression pin on the model itself: the exact estimate for the two known
        // shapes. Unlike the measured comparison below (both sides frozen
        // literals), this fails the moment the model's output changes for a shape
        // we have documented numbers for — which is what makes a model change
        // deliberate.
        let standard = shape(5184, 3599, true); // a roll frame, 18.66 MP HDRi
        for (shape, profile, sampling, accounted, estimated) in [
            // 74.65 MP: decode and encode 18 B/px, decode-only 18 B/px (explicit base).
            (
                big(),
                convert_u16(),
                SamplePlan::none(),
                1_343_692_800u64,
                1_679_464_448u64,
            ),
            (
                big(),
                RunProfile::DecodeOnly,
                SamplePlan::none(),
                1_343_692_800,
                1_679_464_448,
            ),
            // 18.66 MP.
            (
                standard,
                convert_u16(),
                SamplePlan::none(),
                335_829_888,
                520_422_086,
            ),
            (
                standard,
                RunProfile::DecodeOnly,
                SamplePlan::none(),
                335_829_888,
                520_422_086,
            ),
            // 18.66 MP gain map: encode 41 B/px.
            (
                standard,
                RunProfile::GainMapJpeg,
                SamplePlan::none(),
                764_945_856,
                1_013_905_454,
            ),
        ] {
            let e = estimate_peak(&shape, profile, sampling).unwrap();
            assert_eq!(e.accounted_bytes, accounted, "{profile:?}");
            assert_eq!(e.estimated_peak_bytes, estimated, "{profile:?}");
        }
    }

    #[test]
    fn estimate_stays_conservative_against_the_measured_peaks() {
        // The calibration sets from the module doc. The model must never come in
        // *under* a measured peak — an under-estimate would let the gate approve a run
        // that then OOMs.
        //
        // Documentation of intent, not a live check: both sides are frozen
        // literals, so this can never fail on any target. The regression pin on the
        // model's own output is `estimate_pins_the_model_output_for_the_calibration_shapes`.
        // Every measured conversion used an explicit `--film-base`; keep the
        // corresponding no-sampling plan alongside each frozen case.
        //
        // The conversions, after `nf-core/release-decoded-image` (the SDR TIFF for
        // `U16Tiff`, the linear float TIFF for `F32Tiff`, and one frame of
        // `measure-roll`): Linux on synthetic HDRi, then macOS (the highest run) on
        // real HDRi and the 2x2 tile. The SDR TIFF and the gain map were re-measured on
        // macOS after `nf-core/buffer-strategy` dropped the IR plane at the decode.
        let small = shape(2700, 2160, true); // 5.83 MP HDRi
        let small_mac = shape(1890, 3083, true); // 5.83 MP HDRi
        let large = shape(5184, 3600, true); // 18.66 MP HDRi
        let (u16_out, f32_out, gain_map) =
            (convert_u16(), RunProfile::F32Tiff, RunProfile::GainMapJpeg);
        let conversions = [
            (small, f32_out, 111_693_824u64),
            (large, f32_out, 342_638_592),
            (big(), f32_out, 1_350_287_360),
            (small, gain_map, 220_876_800),
            (large, gain_map, 692_514_816),
            (big(), gain_map, 2_750_021_632),
            (small, RunProfile::MeasureRoll, 112_050_176),
            (large, RunProfile::MeasureRoll, 342_818_816),
            (big(), RunProfile::MeasureRoll, 1_350_672_384),
            (small_mac, u16_out, 116_391_936),
            (large, u16_out, 347_422_720),
            (big(), u16_out, 1_355_202_560),
            (small_mac, f32_out, 115_425_280),
            (large, f32_out, 346_587_136),
            (big(), f32_out, 1_354_285_056),
            (small_mac, gain_map, 220_758_016),
            (large, gain_map, 696_844_288),
            (big(), gain_map, 2_747_416_576),
            (small_mac, RunProfile::MeasureRoll, 115_261_440),
            (large, RunProfile::MeasureRoll, 346_095_616),
            (big(), RunProfile::MeasureRoll, 1_353_940_992),
        ];
        // Decode-only: macOS/aarch64, on real scans.
        let decode_only = [
            (big(), RunProfile::DecodeOnly, 1_503_330_304u64),
            (shape(5184, 3599, true), RunProfile::DecodeOnly, 328_286_208),
        ];
        for (shape, profile, measured) in conversions.into_iter().chain(decode_only) {
            let e = estimate_peak(&shape, profile, SamplePlan::none()).unwrap();
            assert!(
                e.estimated_peak_bytes >= measured,
                "{profile:?} at {}x{}: estimate {} under measured {measured}",
                shape.width,
                shape.height,
                e.estimated_peak_bytes
            );
        }

        // …and not wildly over, where being over actually matters: on the full-size
        // frames (the only ones near a plausible budget) the estimate stays within
        // 25% of measured. Small frames are deliberately looser — the fixed
        // allowance dominates them (see the module doc).
        for (profile, measured) in [
            (RunProfile::DecodeOnly, 1_503_330_304u64),
            (u16_out, 1_355_202_560),
            (f32_out, 1_350_287_360),
        ] {
            let e = estimate_peak(&big(), profile, SamplePlan::none()).unwrap();
            assert!(
                e.estimated_peak_bytes < measured / 100 * 125,
                "{profile:?}: estimate {} more than 25% over measured {measured}",
                e.estimated_peak_bytes
            );
        }
    }

    #[test]
    fn absurd_dimensions_overflow_loudly_instead_of_wrapping() {
        // A corrupt header must not wrap into a small, permissive estimate.
        let huge = shape(u32::MAX, u32::MAX, true);
        let err = estimate_peak(&huge, convert_u16(), SamplePlan::none()).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert!(err.to_string().contains("overflow"), "{err}");
    }

    #[test]
    fn over_budget_is_a_resource_error_naming_both_numbers() {
        let budget = Budget::resolve(Some(1024 * 1024 * 1024)); // 1 GiB
        let err = preflight(&big(), convert_u16(), SamplePlan::none(), budget, None).unwrap_err();
        assert_eq!(err.exit_code(), 6);
        let msg = err.to_string();
        assert!(msg.contains("--max-memory"), "{msg}");
        assert!(msg.contains("1.0 GiB"), "{msg}");
        assert!(msg.contains("10368x7200"), "{msg}");
        assert!(msg.contains("smaller frame"), "{msg}");
    }

    #[test]
    fn minimum_viable_budget_is_the_fixed_allowance() {
        // The fixed allowance is unconditional, so it is a floor: a 1x1 image
        // estimates at 128 MiB + its 18 bytes, and any budget at or below the floor
        // rejects *every* possible input. Documented here (and in the rejection
        // message) so the floor isn't a surprise.
        let tiny = shape(1, 1, false);
        let e = estimate_peak(&tiny, convert_u16(), SamplePlan::none()).unwrap();
        assert_eq!(e.estimated_peak_bytes, ALLOWANCE_FIXED_BYTES + 18);

        let err = preflight(
            &tiny,
            convert_u16(),
            SamplePlan::none(),
            Budget::resolve(Some(ALLOWANCE_FIXED_BYTES)),
            None,
        )
        .unwrap_err();
        assert_eq!(err.exit_code(), 6);
        let msg = err.to_string();
        // The advice must not be the impossible "convert a smaller frame" — it must
        // name the fixed floor and say a smaller frame cannot help.
        assert!(!msg.contains("or convert a smaller frame"), "{msg}");
        assert!(msg.contains("no smaller frame fits"), "{msg}");
        assert!(msg.contains("128.0 MiB"), "{msg}");

        // One byte more than the floor plus the frame admits it.
        assert!(
            preflight(
                &tiny,
                convert_u16(),
                SamplePlan::none(),
                Budget::resolve(Some(ALLOWANCE_FIXED_BYTES + 18)),
                None,
            )
            .is_ok()
        );
    }

    #[test]
    fn default_budget_admits_the_largest_real_scan() {
        // The whole point of the fixed default: the biggest scan on hand must pass
        // it, on every machine, without a flag — including with a full-frame
        // `--base-region`, the largest sample a run can gather.
        let frame = big().width as u64 * big().height as u64;
        for sampling in [SamplePlan::none(), SamplePlan::rect(frame)] {
            let report =
                preflight(&big(), convert_u16(), sampling, Budget::resolve(None), None).unwrap();
            assert_eq!(report.budget_source, BudgetSource::Default);
            assert_eq!(report.budget_bytes, DEFAULT_MAX_MEMORY_BYTES);
            assert_eq!(report.decision, Verdict::Ok);
        }
    }

    #[test]
    fn budget_reports_its_own_provenance() {
        // The enum is the single source of both wire keys, so a "default" budget
        // can't report a non-default byte count.
        assert_eq!(Budget::resolve(None).bytes(), DEFAULT_MAX_MEMORY_BYTES);
        assert_eq!(Budget::resolve(None).source(), BudgetSource::Default);
        assert_eq!(Budget::resolve(Some(1234)).bytes(), 1234);
        assert_eq!(Budget::resolve(Some(1234)).source(), BudgetSource::Flag);
    }

    #[test]
    fn warn_tier_fires_only_against_detected_ram() {
        let budget = Budget::resolve(Some(8 * 1024 * 1024 * 1024));

        // A 2 GiB machine: the estimate is over 70% of RAM → warn (but proceed).
        let small = preflight(
            &big(),
            convert_u16(),
            SamplePlan::none(),
            budget,
            Some(2 * 1024 * 1024 * 1024),
        )
        .unwrap();
        assert_eq!(small.decision, Verdict::Warn);
        let msg = warn_message(&small).expect("a warn verdict must carry a message");
        assert!(msg.contains("70%"), "{msg}");
        assert!(msg.contains("2.0 GiB"), "{msg}");

        // A 48 GiB machine: nowhere near the threshold.
        let large = preflight(
            &big(),
            convert_u16(),
            SamplePlan::none(),
            budget,
            Some(48 * 1024 * 1024 * 1024),
        )
        .unwrap();
        assert_eq!(large.decision, Verdict::Ok);
        assert_eq!(warn_message(&large), None);

        // Undetectable RAM disables the tier rather than guessing — and the
        // message guard lives inside `warn_message`, so no caller can render an
        // "unknown of RAM" warning.
        let unknown = preflight(&big(), convert_u16(), SamplePlan::none(), budget, None).unwrap();
        assert_eq!(unknown.decision, Verdict::Ok);
        assert_eq!(unknown.detected_total_ram_bytes, None);
        assert_eq!(warn_message(&unknown), None);
    }

    #[test]
    fn max_memory_parses_units_and_rejects_nonsense() {
        assert_eq!(parse_max_memory("4GiB").unwrap(), 4 * 1024 * 1024 * 1024);
        assert_eq!(parse_max_memory("4G").unwrap(), 4 * 1024 * 1024 * 1024);
        assert_eq!(parse_max_memory("4GB").unwrap(), 4_000_000_000);
        assert_eq!(parse_max_memory(" 512 MiB ").unwrap(), 512 * 1024 * 1024);
        assert_eq!(parse_max_memory("2000000000").unwrap(), 2_000_000_000);
        assert_eq!(parse_max_memory("1b").unwrap(), 1);

        for bad in ["", "GiB", "4 GiBs", "-1", "4.5GiB", "0", "0GiB"] {
            let err = parse_max_memory(bad).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{bad:?} should be a usage error");
        }
        assert_eq!(parse_max_memory("99999999TiB").unwrap_err().exit_code(), 2);
    }

    #[test]
    fn detected_ram_is_plausible_or_absent() {
        // Fail-soft: `None` is allowed (unsupported platform), but a reported
        // value must be sane — a bogus tiny number would make the warn tier fire
        // on every run.
        if let Some(ram) = detect_total_ram() {
            assert!(
                ram >= 256 * 1024 * 1024,
                "implausible detected RAM: {ram} bytes"
            );
        }
        // The Linux path is compiled on every target; off Linux the files are
        // absent, so it must degrade to `None` rather than misreport.
        if !cfg!(target_os = "linux") {
            assert_eq!(linux_total_ram(), None);
        }
    }

    #[test]
    fn linux_ram_parsers_are_fail_soft() {
        // MemTotal is read in kB and may not be the first line.
        assert_eq!(
            parse_meminfo_total("MemFree:  1 kB\nMemTotal:       16384 kB\n"),
            Some(16384 * 1024)
        );
        for bad in [
            "",
            "MemTotal:\n",
            "MemTotal: lots kB\n",
            "SwapTotal: 4 kB\n",
        ] {
            assert_eq!(parse_meminfo_total(bad), None, "{bad:?}");
        }

        // A cgroup ceiling is honored; "no limit" in either cgroup dialect is not.
        assert_eq!(
            parse_cgroup_limit("2147483648\n"),
            Some(2 * 1024 * 1024 * 1024)
        );
        assert_eq!(parse_cgroup_limit("max\n"), None, "cgroup v2 unlimited");
        assert_eq!(
            parse_cgroup_limit("9223372036854771712\n"),
            None,
            "cgroup v1 unlimited sentinel"
        );
        for bad in ["", "0", "not-a-number"] {
            assert_eq!(parse_cgroup_limit(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn cgroup_paths_resolve_the_process_s_own_nested_cgroup() {
        // The limit that binds a containerized run lives at the process's *own*
        // cgroup, which under Kubernetes/systemd is nested. Reading only the root
        // files reports `max` there and leaves the warn tier silent in exactly the
        // environment it exists for. Runs on every target — this logic is Linux-only
        // in effect but not in compilation, so CI is not the first place it builds.
        let k8s = "0::/kubepods.slice/kubepods-burstable.slice/pod123/container456\n";
        let (v2, v1) = parse_self_cgroup(k8s);
        assert_eq!(
            v2.as_deref(),
            Some("/kubepods.slice/kubepods-burstable.slice/pod123/container456")
        );
        assert_eq!(v1, None);

        // v1: the memory controller's own line, among others.
        let v1_text = "12:memory:/docker/abc\n11:cpu,cpuacct:/docker/abc\n0::/\n";
        let (v2, v1) = parse_self_cgroup(v1_text);
        assert_eq!(v2, None, "a root v2 path is not nested");
        assert_eq!(v1.as_deref(), Some("/docker/abc"));

        // Ancestors, most specific first, ending at the root.
        assert_eq!(
            ancestors_of("/kubepods/pod1/ctr"),
            vec!["/kubepods/pod1/ctr", "/kubepods/pod1", "/kubepods", ""]
        );
        assert_eq!(ancestors_of("/"), vec![""]);

        // The candidate list puts the process's own file first and still ends with
        // the root fallbacks, so a non-nested host keeps working.
        let paths = cgroup_limit_paths();
        assert!(
            paths.last().map(String::as_str) == Some("/sys/fs/cgroup/memory/memory.limit_in_bytes"),
            "root v1 fallback must remain last: {paths:?}"
        );
        assert!(
            paths.contains(&"/sys/fs/cgroup/memory.max".to_string()),
            "root v2 fallback must be present: {paths:?}"
        );
    }

    #[test]
    fn an_out_of_bounds_rectangle_cannot_become_a_resource_rejection() {
        // A typo'd `--base-region 0,0,999999,999999` is a *usage* error that
        // `film_base::region_channels` reports (exit 2). Estimating its raw `w*h`
        // would instead reject the run as a 12852 GiB resource overrun (exit 6), or
        // overflow the model and blame the image's own dimensions. The sample is
        // clamped to the frame, so the gate stays quiet and the real error surfaces.
        let s = shape(502, 462, false);
        let frame = 502u64 * 462;
        let absurd = estimate_peak(
            &s,
            RunProfile::DecodeOnly,
            SamplePlan::rect(999_999 * 999_999),
        )
        .unwrap();
        let clamped = estimate_peak(&s, RunProfile::DecodeOnly, SamplePlan::rect(frame)).unwrap();
        assert_eq!(
            absurd.film_base_bytes, clamped.film_base_bytes,
            "an oversized rectangle must estimate as at most the whole frame"
        );
        // …including one big enough to overflow the model if left unclamped.
        let huge =
            estimate_peak(&s, RunProfile::DecodeOnly, SamplePlan::rect(u64::MAX / 2)).unwrap();
        assert_eq!(huge.film_base_bytes, clamped.film_base_bytes);
    }

    #[test]
    fn report_serializes_flat_with_the_documented_keys() {
        let report = preflight(
            &shape(1000, 1000, true),
            convert_u16(),
            SamplePlan::rect(500 * 500),
            Budget::resolve(None),
            Some(48 * 1024 * 1024 * 1024),
        )
        .unwrap();
        let json = serde_json::to_value(report).unwrap();
        for key in [
            "estimated_peak_bytes",
            "accounted_bytes",
            "decode_bytes",
            "film_base_bytes",
            "render_bytes",
            "encode_bytes",
            "budget_bytes",
            "budget_source",
            "decision",
            "detected_total_ram_bytes",
        ] {
            assert!(json.get(key).is_some(), "missing report key {key}");
        }
        assert_eq!(json["budget_source"], "default");
        assert_eq!(json["decision"], "ok");
    }
}
