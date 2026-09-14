# Negative Converter — output Progress Log

Execution log for the `output` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status (the checkboxes);
this file is the narrative beside it.

One `##` section per task in this epic, named by the bare task name (the part
after the `/`). Read this whole file before starting a task in this epic, and
read other epics' `Epic summary` sections when you depend on them. Append
entries — don't rewrite earlier ones.

> **Consolidated 2026-09-13.** Sections of tasks that were done were collapsed into
> one section each, keeping the decisions and measurements that still matter and
> dropping review-round narration. The verbatim history is in git
> (`git log -- docs/progress/output.md`, before this date). Sections of open tasks
> are unchanged.

## Epic summary

What other epics need to know about `output`:

- **The preset surface is complete (2026-08-09, `output/presets`).** Twelve names
  are accepted, enumerated once in `OutputPreset::ALL`; `gain-map-hdr` is the
  default at `pipeline_version` 3. `custom` is the one named preset that is not
  atomic (`is_atomic()`, not `is_named()`, gates atomicity — three call sites).
  `--output-hdr`/`--output-sdr`/`output.hdr` are gone, replaced by
  `--out-depth u16|f32` / `output.depth`. **Every preset is roll-capable**: roll
  derives `<stem>_positive.<ext>` from the frame's own resolved preset via
  `cli::derived_extension` (not the head of `required_extensions`, which lists
  `tif` first and would rename every existing `_positive.tiff`), and an explicit
  manifest `output` goes through the same `reject_suffix_mismatch` rule `convert`
  uses. A bare `nc convert -o out.tif` with no preset is exit 2 by design, and an
  extensionless `-o` is rejected too (`output/output-path-suffix` proposes to relax
  the latter). Measured in
  [reports/render-defaults-v3.md](../reports/render-defaults-v3.md).
- **The HDR spike is closed and its numbers are binding.** ISO 22028-5:2026 and
  ISO 21496-1:2025; **203 cd/m² reference white**, **1000 cd/m² target peak**,
  4.926108 linear and 2.300448 log2 capacity of that display ratio (not
  per-pixel gain extrema — those come from the offset-adjusted formula). The
  renderers **may not change reference white, target peak, the common gain-map
  domain, or the RGB-map decision** without reopening `docs/hdr-output-spike.md`.
  The spike waived the licensed-normative-text review at spike level and re-homed
  it as a pre-merge gate on the encoder tasks; the ISO 21496-1 half was discharged
  by buying the text (2026-08-04), the AVIF/AV1 half by reading the public specs.
- **Containers:** JPEG + ISO 21496-1 gain map is the default HDR still; 10-bit
  4:4:4 BT.2020 AVIF is the explicit PQ/HLG path; three HDR TIFFs are interchange
  encodings. HEIC is deferred (no portable encoder API for the final gain-map
  container, plus HEVC licensing risk).
- **`gain-map-hdr` and `ultra-hdr-v1` are one render packaged twice**, differing
  only in dialect (`Dialects::LegacyPlusIso` vs `LegacyUltraHdrV1`), which rides in
  `FrameRender::UltraHdr`. **`ultra-hdr-v1` is not HDR on Apple platforms —
  measured, not inferred (2026-08-06, re-confirmed on the CLI's own output
  2026-08-09).** Apple ignores Google's legacy Ultra HDR v1 XMP entirely, so that
  preset's file opens as an ordinary SDR JPEG on macOS/iOS. Only the ISO dialect is
  read there, which is why the default is dual-dialect. What remains of
  `output/gain-map-dialect-activation` is Android 15+ verification only; its CLI
  half was consumed by `output/presets`.
- **Verify gain-map output with `scripts/iso-decoder-oracle/`** (Apple ImageIO,
  macOS-only, not in CI). exiftool and libultrahdr both accept a file no decoder
  parses — that is exactly how a placement defect shipped. Two traps when using
  it: the sample set needs `NC_ISO_SAMPLE_EV=3.0` or the gain map is inert, and
  the reported `headroom 4.9261084` is nc's own declared `1000/203` echoed back
  rather than a measurement — it reads the same on a flat map, so the pass
  condition is `PRESENT` **plus** a `GainMapMax` above 0.
- **The default's gain map is inert, and that is an accepted state, not a bug to
  fix here (2026-08-10).** `GainMapMax` decodes as 1.0x on every measured roll,
  because under the default sigmoid the HDR rendition peaks at exactly the 203-nit
  reference white. The cause is the *curve*, not the container: the sigmoid's
  shoulder strips above-white content before either display branch sees it
  (`--sigmoid-shoulder 0` alone reaches 4.87x, as does the exponential). Two
  consequences: **(a)** do not read a flat gain map as an `output` defect, and do
  not add headroom in the gain-map stage to compensate; **(b)** HDR is explicitly
  lower priority and must not block the sigmoid path, but the HDR presets stay
  first-class and `gain-map-hdr` stays the default so the capability stays
  exercised. The warning that *would* say "this frame's gain map is flat" does not
  exist: `hdr::sdr_range_warning` is single-rendition only and the gain-map pair
  deliberately does not get it. `algo/reconstruction-render-curve-split` **settled**
  (2026-09-02) that tone shaping should move out of reconstruction, but **no default
  has moved** — activation is `algo/split-default-migration`.
- **The display tone curve is selectable** (`print.display_tone` /
  `--display-tone <shoulder|none|reinhard>`, `output/linear-render` 2026-09-01 plus
  `output/display-tone-mapping` 2026-09-02) and **every display preset accepts all
  three**, including the gain-map pair; only `legacy`/`custom`/`film-master` refuse.
  The default is unchanged (`shoulder`), so `pipeline_version` stayed 3. `none` is
  **self-policing** rather than gated on a curve type: a render exceeding the
  branch's ceiling fails loudly mid-render, exit 1, and those ceilings **differ**
  (reference white for SDR, the 1000-nit peak for HDR), so the same overshoot is
  refused on `display-p3` and renders on `hdr-pq`. `reinhard` is
  `extended-reinhard-mid-preserving-v2` since **2026-09-09**: it preserves scene
  mid-grey at every headroom, `headroom_stops` (default 6, display-referred) is the
  curve's *scale*, and diffuse white costs ≈0.86 stop on both branches. Anything
  holding a `reinhard` rendition from before that date must re-render. The gain-map
  pair was admitted only because `gain_map::build` ratios against the base **as
  stored** (`min(sdr, 1)`); never relax the check instead.
- **Native dependency packaging:** the shipped Ultra HDR implementation keeps
  the audited libultrahdr/libjpeg-turbo snapshot in-tree. **The plan changed on
  2026-08-05** (`output/ultrahdr-dependency-externalization`, id kept, scope now
  *removal*): the exit is nc writing the Ultra HDR v1 XMP and MPF container in
  Rust so the C/C++ dependency leaves the tree, **not** swapping in a published
  crate. Two facts for anyone touching this: our snapshot's
  `libultrahdr/CMakeLists.txt` is the **one file modified** from upstream
  `11ac0c3`, and only **6** native calls are on the shipping path — the rest of
  the `uhdr::` surface is the test-only decode oracle. `UHDR_MAX_DIMENSION` is
  raised to 65500 through `ultrahdr-sys`'s `jpeg-max-dimension` feature (no
  vendored source patched); at its 8192 default packaging refuses real 5000 dpi
  scans after the full render.
- **The AVIF path does *not* use libavif** (decided 2026-08-05 in
  `hdr-avif-output`): no published crate ships libavif ≥ 1.4.2 and
  `avif-serialize` cannot emit `MA1A`, so it is published `libaom-sys` for the
  codestream plus an nc-owned Rust MIAF/AVIF container writer. `av1C` is filled by
  **parsing the encoded sequence-header OBU**, never from the encoder config.
  Windows static builds are deferred → `output/hdr-avif-windows-packaging`.
- **`hdr-linear-tiff` is the display-linear HDR interchange master**: it is not
  `film-master` (linear ACEScg *before* display rendering), not `hdr-pq`/`hdr-hlg`
  (no transfer applied), and not `--out-depth f32` on `legacy` (print-rendered
  float in the selected output space). It writes `pipeline::hdr::render_linear`'s
  pre-transfer BT.2020/D65 samples verbatim as unclamped f32. The **report block and
  sidecar `meta`, not the ICC profile, are authoritative** for reference white /
  peak / headroom. **`hdr-pq-tiff` / `hdr-hlg-tiff`** store the same rendition as
  the AVIF presets as full-range 16-bit codes; they are **limited-interoperability
  interchange, never display-ready** (only a CICP-aware reader honours the ICC
  `cicpTag`), and their profiles are conformant Display-class since 2026-08-09.
- **Which memory phase peaks is per profile, and measured.** `HdrLinearTiff`,
  `HdrCodedTiff`, `SdrTiff`, `UltraHdrV1`/`GainMapHdr` all peak at **render**;
  `HdrAvif` and `Convert` at encode. Read it off
  `pipeline::memory`'s `which_phase_peaks_is_per_profile_and_measured_not_assumed`,
  never off a category — prose about it has been wrong three times.
- **`definitions::BT2020` is fed to Little CMS** by `color::hdr_linear_bt2020_icc`,
  making **five** lcms2-consumed colour spaces (`REC709`, `DISPLAY_P3`, `ACESCG`,
  `PROPHOTO`, `BT2020`). Editing any of the five changes embedded ICC bytes and
  lcms2-transformed pixels *even with `pinned.rs` untouched and every audit ulp at
  0*, and nothing automated catches it.
- **ICC authoring gotchas** (all from the coded-HDR profiles): ICC PCSXYZ in a LUT
  tag is `u1Fixed15Number` (`1.0` → `0x8000`), so an A2B matrix is pre-divided by
  `32768/65535` or every luminance comes out 2×; Little CMS serializes `mAB ` only
  for a recognized stage pattern (M curves → Matrix → B curves, identity B curves
  mandatory); `definitions::ICC_PCS_WHITE_XYZ` is ICC's *declared*
  `[0.9642, 1, 0.8249]`, not `D50.to_xyz()` — anything serializing a profile adapts
  to the declared triple; and `pinned::BT2020_TO_XYZ_D50` / `XYZ_D50_TO_BT2020` /
  `BRADFORD_D65_TO_ICC_PCS` exist because nc authors that profile itself, each
  anchored on a relationship (`A·B == I`, `chad · NPM == colorants`).
- **Ownership split — read this before touching a transform.**
  `output/display-p3-output` owns only the *destination encoding* (synthesized
  Display P3 ICC + parametric sRGB TRC). `output/sdr-display-rendering` owns
  ACEScg → rendered-linear destination RGB: reference white, tone, chromatic
  adaptation, gamut mapping. Renderers return **rendered-linear** pixels; transfer
  encoding happens afterward. Gain-map construction consumes the *pre-transfer*
  rendition so ratios are taken in a common linear domain.
- **The HDR renderer** (`pipeline::hdr::render_linear`) returns finite,
  non-negative, reference-white-relative BT.2020 pixels; `encode_transfer` consumes
  them in place and returns opaque Rec.2100 PQ or HLG pixels plus the full-range
  CICP 9/16/9 or 9/18/9 contract. HLG pins the 1000-nit, zero-black reference OOTF
  with system gamma 1.2. `clli` is **measured** per frame (MaxCLL/MaxFALL from the
  display-linear pixels), never the 1000/203 policy constants.
- **Gain-map math is pinned:** per-channel `(HDR + offset_hdr) / (min(SDR, 1) +
  offset_sdr)` in common linear Display P3 normalized by 203 cd/m². Extrema come
  from actual per-pixel values over independently tone-mapped renditions. No
  arbitrary epsilon, no silent clamp, no `0/0` — those are fail-loud cases. The
  `min(SDR, 1)` is **not** a clamp of the kind that sentence forbids (2026-09-02).
- **The preset/`RunProfile` ownership rule:** whichever task ships an explicit
  preset also adds and calibrates that preset's `memory::RunProfile`, on two frame
  sizes, leaving `accounted` slightly under measured.
- **ICC bytes are platform-dependent**, so profile-inclusive byte hashes are not a
  valid cross-platform gate — profile determinism here is pinned per build via the
  dateTime-zeroing path.


## display-p3-output
**Status:** done (2026-07-24, PR #50)

Added `OutputSpace::DisplayP3` as a `--output-profile` / `output.output_profile`
keyword (`display-p3` / `displayp3`) on the existing string knob — no new field or
merge arm. The profile is synthesized with Little CMS from the registered P3
encoding (D65 white 0.3127/0.3290; R 0.680/0.320, G 0.265/0.690, B 0.150/0.060) plus
a **parametric type-4** sRGB TRC (`srgb_trc()`), never a gamma-2.2 approximation.
Verified empirically that `Profile::new_rgb(D65, P3, srgb_curve)` produces an ICC
v4.4 Display-class profile that itself writes D50 media white, the
`chromaticAdaptationTag` and Bradford D65→D50 colorants matching the ICC
registry / macOS reference (rXYZ 0.51512/0.24119/-0.00105) — no manual chad or
colorant handling. Determinism reuses the `profile_icc` dateTime-zeroing path.

On the legacy path this profile is reached from linear Rec.709 working values, so
`--output-profile display-p3` there is a lossless Rec.709→P3 remap plus the sRGB
TRC; the pure transfer-encode of already-rendered linear P3 is what
`sdr-display-rendering` / the `display-p3` preset do.

Known cosmetic wart, unowned: this profile (like every other nc matrix-shaper
profile) carries Little CMS's default `ProfileDescription: "RGB built-in"`. Only the
three HDR TIFF profiles were given real names (2026-08-06); renaming the older ones
changes already-shipped ICC bytes and was left for a deliberate decision.


## hdr-output-spike
**Status:** done (2026-07-24)

Decision note: [`docs/hdr-output-spike.md`](../hdr-output-spike.md). Pinned ISO
22028-5:2026 (replacing the withdrawn 2023 TS), ISO 21496-1:2025, BT.2100-3,
203 cd/m² reference white, 1000 cd/m² target peak, 4.926108 linear / 2.300448 log2
headroom; JPEG + ISO gain map as the default HDR still, 10-bit 4:4:4 AVIF for
explicit PQ/HLG, HEIC deferred. Prototype PQ/HLG AVIF decoded in ImageIO with the
expected headroom; prototype libultrahdr 1.4.0 JPEGs were rejected by ImageIO on
marker order (fixed upstream in PR 394, merged 2026-07-27 as `11ac0c3`).

Closed **without** the licensed normative text: completion gate 1 was waived at
spike level and re-homed to the encoder tasks as a pre-merge conformance gate.
(The task file's "remains open only for the normative-text review" sentence
predates that closure.) The spike's encoder paragraph was later amended by
`hdr-avif-output` (no libavif) — its *numbers* were not.


## sdr-display-rendering
**Status:** done (2026-07-28, PR #61)

`pipeline/sdr.rs`: accepts only the typed shared adjusted ACEScg source
(`film-master-render-pipeline`'s split), uses pinned AP1/D60 → P3-D65 / sRGB-D65
matrices, maps adjusted `1.0` to the 203 cd/m² reference white, applies a resolved
Hermite highlight shoulder, and maps out-of-gamut colour **radially toward the
same-luminance neutral axis** rather than clipping channels independently. Output
is finite, non-negative rendered-linear RGB plus serialized policy metadata; any
non-finite or out-of-range postcondition fails with the pixel index. Named-SDR
`highlight_compress` is bounded to a 0.75 baseline / 0.5 limiting knee (the
`0.5 + 0.25/(1+hc)` formula now lives in `display_tone`). `color::encode_rendered_sdr`
is the destination seam: rendered-linear P3 gets only the sRGB transfer + Display P3
profile; rendered-linear sRGB gets sRGB; neither re-runs the legacy gamut transform.
Product activation (`display-p3` / `compatibility`) was left to `output/presets`
and shipped 2026-08-09.


## hdr-display-rendering
**Status:** done (2026-07-29, PR #62)

`pipeline::hdr`: the linear seam maps adjusted ACEScg/D60 into BT.2020/D65,
preserves adjusted `1.0` as 203-nit reference white, and (originally) applied a
bounded C¹ Hermite shoulder to the 1000-nit / 4.926108-linear peak — since
2026-09-02 the tone is selectable, see `display-tone-mapping`. Out-of-gamut colour
intersects the BT.2020 cube radially at constant luminance. The transfer seam
mutates in place: PQ applies the ST 2084 inverse EOTF in absolute nits; HLG applies
the inverse reference OOTF (1000-nit peak, zero black, system gamma 1.2), a
scene-linear radial signal-boundary intersection, and the reference OETF. Typed
metadata fixes full-range CICP 9/16/9 (PQ) or 9/18/9 (HLG). The pre-transfer typed
BT.2020 value is borrowable by the gain-map stage, which must convert it to common
linear Display P3 before ratios; the encoded pair feeds `io::avif` and the coded
TIFFs. Review made the encoded seam an opaque nonlinear image type.


## gain-map-hdr-output
**Status:** done (2026-07-30, PR #63; CI follow-up 2026-07-31)

Shipped the explicit `ultra-hdr-v1` preset: quality-95 4:4:4 Display P3 SDR
primary plus a half-resolution **grayscale luminance** gain map in the public
legacy Ultra HDR v1 XMP/MPF/GContainer dialect, no ISO marker, no ISO claim. The
canonical internal model stays **RGB** in common linear Display P3 (for the ISO
serializer); this preset derives luminance because legacy XMP cannot signal a
multichannel map. Offsets `1/64`, gamma `1`, extrema from actual per-pixel values,
centre-aligned 2x downsample. The container/ISO split was made 2026-07-30 with user
approval, after a stale cached GitHub page had wrongly reported upstream PR #394
still open — a live `gh pr view` showed it merged 2026-07-27.

Facts `output/ultrahdr-dependency-externalization` inherits:

- google/libultrahdr pinned at `11ac0c325bbf56ecf8be8704ff0f79fc9e1aac77`,
  libjpeg-turbo 3.1.0 at `20ade4dea9589515a69793e447a6c6220b464535`, snapshot
  verified by `scripts/check-vendored-native.py` (219 + 555 files). The copied
  upstream `.gitignore` made ordinary `git add` omit 20 legitimate files, so those
  are **force-tracked** and the verifier checks every hashed file is in the index.
  Snapshot, guard and verifier go together.
- `patches/libultrahdr-no-threads.patch` is applied at build time.
- Distribution carries the Adobe notice ("This product includes Gain Map technology
  under license by Adobe") in `THIRD_PARTY_NOTICES.md`.
- `RunProfile::UltraHdrV1` calibrated on an 18.7 MP HDRi scan: estimate
  1,851,158,528 against measured 1,681,408,000 peak RSS. Its 20 B/px `byte_staging`
  term is explicitly libultrahdr's copies, and must be re-measured when assembly
  moves to Rust.
- Physical Android verification never ran (conditional on an environment) —
  carried by `output/gain-map-dialect-activation`.


## iso-gain-map-metadata
**Status:** done (2026-08-07, PR #76 + #81 + #82)

Landed nc-serialized ISO 21496-1 C.2.2 metadata in both JPEG images
(`pipeline/gain_map/iso.rs`; C.4.3 version-only in the baseline, C.4.6 full
structure in the gain map), placed in the **header block**, verified by Apple
ImageIO reading every field back as written and by libultrahdr still decoding the
legacy dialect from the same file. `Dialects::{LegacyUltraHdrV1, LegacyPlusIso}`
and `encode_with` carry it; `io::ultra_hdr::assemble` is the **single** container
path both the product and the oracle's test fixture use, so a marker change cannot
leave the oracle measuring a container nc no longer ships. Shipped without two of
its own verification bullets, by user call: Android 15+ was never exercised, and
there was no CLI path (the latter was consumed by `output/presets`'
`gain-map-hdr` on 2026-08-09; the Android half is
`output/gain-map-dialect-activation`).

Durable facts, most of them inherited by `ultrahdr-dependency-externalization`,
`mp-container-conformance` and `gain-map-dialect-activation`:

- **The licensed text was bought (2026-08-04) and paid for itself twice.** First,
  it overturned this log's own earlier claims: `urn:iso:std:iso:ts:21496:-1` **is**
  the published first edition's identifier (C.3 / C.4.6, 27 chars + NUL = 28
  bytes), so libultrahdr was never "on a draft namespace"; anyone re-deriving this
  from an implementation's URN will reach the same wrong conclusion, and
  `segment_label_matches_the_published_length_and_identifier` guards it. Second, it
  found a real defect in the reference implementation: C.2.2 has **no
  common-denominator compact form and no `backwardDirection` bit** (flag bits 5..0
  are reserved), and libultrahdr emits both whenever all denominators match — which
  nc's uniform `1/64` offsets and `gamma = 1` make the *common* case. That is why nc
  owns its serializer; never add the compact form.
- **Two things the task set out to pin are not in the standard.** ISO 21496-1 is
  silent on Google's XMP dialect (the ISO ↔ Ultra HDR v1 mapping is nc's to define)
  and on dual-dialect precedence ("prefer ISO" traces to Android guidance), so
  precedence is *observed decoder behaviour*, never a conformance claim. C.2.3 also
  settles that `is_multichannel` describes the **metadata** channel count and may
  differ from the map's — nc always writes 3 over the achromatic map, ImageIO parses
  all three, do not "fix" it.
- **libultrahdr's `package()` is asymmetric** — established by probe, and easy to
  get backwards: it **rewrites** the baseline's marker segments (dropping unknown
  APP2s) and **appends** the gain-map image **verbatim**. So the gain map's segment
  goes in at encode time (`jpeg_encoder::add_app_segment`) and the baseline's is
  spliced in after packaging by `insert_baseline_iso_segment`. That splice must
  satisfy **two** constraints at once: before `SOF0` (an `APPn` scan stops at the
  frame header) and before the `MPF\0` label (MPF offsets are relative to the byte
  after it, so inserting there keeps every stored offset valid and only the first
  image's recorded size is patched). The first version satisfied only the second
  — see the decoder-oracle section below. Fix:
  `leading_app_segment_end(packaged)?.min(mpf_start)`, pinned by
  `baseline_iso_segment_precedes_the_frame_header`.
- The canonical gain form and the standard agree by construction:
  `gain_matches_the_standards_application_formula_round_trip` recovers the HDR
  rendition through Clause 6.3's `Alternate = (Baseline + k_base) · 2^(W·G) − k_alt`,
  confirming nc's linear ratios and the standard's log2 `G` are one model and that
  the reference-white-relative common domain is the standard's application space.
- **Resampling phase stays centre-aligned** (6.2.2 NOTE 1 prefers co-sited, but the
  NOTE is informative and switching would change shipped `ultra-hdr-v1` bytes);
  recorded on `gain_map::resample_axis`, shared by both dialects.
- **Exif tripwire:** `baseline_carries_no_exif_colorspace_claim`. C.4.4 branches on
  Exif ColorSpace; with no Exif and an ICC present (branch two) the ICC governs.
  Whoever adds Exif (`mp-container-conformance`) must write `Uncalibrated`, never
  `1`, which would force an sRGB reading of the Display P3 base.
- ISO metadata bytes pass through `log2` and a continued-fraction rational
  approximation, so they are **per-build/architecture** — never pin them with a
  cross-platform hash.
- Shipped `ultra-hdr-v1` bytes were unchanged throughout (sha256 `67911f22…5540` on
  the Ektar reference frame) — that is the standing check.
- **The default render produced no HDR** under the exponential curve either
  (`GainMapMax` 1.0027x at defaults; `--print-exposure +3` is what the oracle files
  use) — first measured here, later confirmed under the sigmoid default by
  `output/presets`.

## iso-gain-map-metadata (decoder oracle — a real defect)

**Status:** in progress
**Updated:** 2026-08-06

- 2026-08-06: **The external decoder oracle ran, and it found a shipping bug the
  entire in-repo suite could not.** The oracle is Apple ImageIO on macOS 26.5
  (`kCGImageAuxiliaryDataTypeISOGainMap`, available since macOS 15.0) — an
  independent ISO 21496-1 implementation, so no device was needed after all.
  Harness: `scripts/`-free, a ~100-line Swift program in the scratchpad plus the
  new `iso_oracle_samples` ignored test.
- 2026-08-06: **The finding: nc's baseline ISO segment was placed where no JPEG
  reader looks.** `insert_baseline_iso_segment` inserted the C.4.3 version-only
  segment "immediately before the MPF segment" — which satisfies every MPF
  invariant and is exactly what the 2026-08-04 entry above reasoned out. But
  libultrahdr emits MPF **after `SOF0` and the tables**, and an `APPn` scan stops
  at the frame header. The bytes were well-formed, correctly sized, MPF-safe —
  and never parsed. ImageIO reported *no gain map at all* and decoded plain SDR.
  The marker order it was written into:
  `SOI · APP0 JFIF · APP1 XMP · APP2 ICC · **SOF0** · APP2 ISO · APP2 MPF · SOS`.
- 2026-08-06: **Isolated by bisection on the bytes, not by reading source.** Three
  hypotheses were tested by patching a produced file and re-running the oracle:
  clearing `use_base_colour_space`, collapsing 3 metadata channels to 1 (with the
  MPF second-image size repaired), and both — **all three still ABSENT**. Moving
  the *unmodified* segment into the header block made it PRESENT immediately. Four
  positions then all worked (after SOI, after JFIF, after XMP, after ICC), which
  pins the boundary as `SOF0` and nothing else.
- 2026-08-06: **`is_multichannel = true` is vindicated, and that was the surprise.**
  The 2026-08-04 decision to always write 3 metadata channels over an achromatic
  map (C.2.3 permits the counts to differ) was the leading suspect. ImageIO parses
  all three and reports them individually. Do not "fix" it.
- 2026-08-06: Fix is `leading_app_segment_end(packaged)?.min(mpf_start)` — the end
  of the leading `APPn` run, clamped to the MPF start in case a future libultrahdr
  emits MPF earlier. That satisfies **both** constraints at once and keeps JFIF
  first. Pinned by `baseline_iso_segment_precedes_the_frame_header`, which was
  confirmed falsifiable (restoring the old insertion point fails it with the marker
  sequence printed). `marker_sequence` gained an `SOF` arm, since ordering against
  MPF alone provably cannot catch this — both markers sat on the wrong side of
  `SOF0` together.
- 2026-08-06: **Verification results on a real 4715x3297 Ektar frame** (dual-dialect
  file, post-fix). ImageIO's independent parse against what nc wrote:

  | field | nc wrote | ImageIO read |
  |---|---|---|
  | GainMapMax (log2) | 9187889/8388608 = 1.09528 | 1.095282 |
  | GainMapMin (log2) | -15127609/268435456 = -0.0563547 | -0.056355 |
  | Gamma | 1/1 | 1.000000 |
  | Base/AlternateOffset | 1/64 | 0.015625 |
  | AlternateHeadroom | log2(1000/203) | 2.300448 |
  | channels | 3 | 3, reported separately |

  HDR reconstruction headroom **4.9261084** = 1000/203, from both
  `CGImageSourceCreateImageAtIndex(kCGImageSourceDecodeToHDR)` and
  `CIImage(.expandToHDR)`. The gain map resolves at 2358x1649 — the ceil-halved
  2x downsample. This closes the "both dialects express the same semantics"
  bullet with an *external* reader rather than by construction.
- 2026-08-06: **Dual-aware precedence is now observed, not assumed: ISO wins.** On
  the conflicting file (legacy XMP `GainMapMax` 1.09528, ISO 2.095282 — one stop
  apart by construction) ImageIO reports **2.095282**. Recorded as *observed Apple
  decoder behaviour*; ISO 21496-1 is silent on coexistence, so this still must
  never be stated as a conformance property.
- 2026-08-06: **Product finding, and it raises the stakes on `output/presets`: the
  shipped `ultra-hdr-v1` preset is not HDR on Apple platforms.** The legacy-only
  file is ABSENT for *both* `kCGImageAuxiliaryDataTypeISOGainMap` and
  `kCGImageAuxiliaryDataTypeHDRGainMap`, and decodes at headroom 1.0. Apple
  ignores Google's Ultra HDR v1 XMP entirely; only the ISO dialect makes the file
  HDR there. The ISO work is therefore not a conformance nicety — it is the only
  thing that makes nc's gain-map output function on macOS/iOS.
- 2026-08-06: Oracle validated against a **known-good control** before any
  conclusion was drawn — `ultrahdr_app` v1.4.0 (homebrew) encoding a synthetic
  rgba1010102 gradient. ImageIO reads that file's ISO metadata and reconstructs at
  4.926 headroom, so ABSENT on nc's files was nc's problem, not the harness's.
  Conversely libultrahdr **accepts all three nc files** (exit 0) where a plain JPEG
  control fails with "does not contain gainmap image" — the legacy dialect and the
  container were always fine. Note the control's payload is 61 bytes (1 channel)
  against nc's 141 (3 channels); both decode to the same layout formula
  `4 + 1 + 16 + 40·channels`, which is independent evidence nc's C.2.2 field order
  is right.
- 2026-08-06: `iso_oracle_samples` (ignored) emits the three-file set the gate
  needs — legacy-only, dual, and conflicting — from one render, so any difference
  is attributable to metadata alone. It renders a **real scan** when
  `NC_ISO_SAMPLE_INPUT`/`_BASE`/`_DMAX`/`_EV` are set. That is not optional
  polish: the toy fixture *and* the default exponential render both produce a flat
  gain map (`GainMapMax` 0.0039 log2 = 1.003x), which cannot discriminate an HDR
  reconstruction. See the separate finding below.
- 2026-08-06: **Separate, larger finding — nc's default render produces no HDR.**
  On a real Ektar frame at defaults the gain map is inert: `GainMapMax` 0.00392
  log2 = **1.0027x**, while the metadata advertises `HDRCapacityMax` 2.30045. The
  file claims 2.3 stops and delivers 0.004. `--highlight-compress` does not move it
  (identical to 6 significant figures at 0, 1 and 4 — the flag *does* resolve, so
  this is content, not plumbing): the exponential curve anchors display white at
  `Dmax` and real content lands far below the SDR shoulder knee, `clipped_high: 0`
  with mean 0.258. Only `--print-exposure +3` pushes content over the knee
  (`GainMapMax` 1.095 log2 = 2.14x), which is what the oracle files use. This is
  the same non-discriminating condition `lossless-hdr-tiff`'s 2026-08-06 viewer
  gate hit and attributed to "diffuse-highlight scene, exponential default curve" —
  it is now measured, and it is the *default*, not the scene. **Belongs to
  `output/presets` / the sigmoid default work, not here**; filed as a note rather
  than fixed, because changing what the default render puts in the gain map is a
  pixel decision this task has no mandate to make.
- 2026-08-06: Shipped `ultra-hdr-v1` output is **byte-identical** across the fix
  (sha256 `67911f22…5540` before and after on the real Ektar frame) — the changed
  path runs only when ISO fields are present. All four gates green: 620 unit + 137
  integration.
- 2026-08-06: **Still open**, unchanged by this pass: C.4.3's CIPA DC-007 baseline
  requirement (the document is still unfetched), and CLI activation, which
  `output/presets` owns. The Android half of the "Android 15+ and target Apple
  software" bullet is also still unrun — Apple is now covered.
- 2026-08-06 (later review rounds): the Swift harness moved into
  `scripts/iso-decoder-oracle/` (macOS-only, not in CI, README covering build,
  sample generation, why `_EV` is required, and how to read the result).
  **Correction to how the result above was first reported: the 4.926 headroom
  figure is not evidence of reconstruction** — it is `2^AlternateHeadroom`, nc's own
  declared `1000/203` echoed back, and reads identically on a flat gain map
  (`GainMapMax = 0.000000`). The discriminating number is `GainMapMax`; the pass
  condition is `PRESENT` **plus** a `GainMapMax` above 0. `iso_sample_for_external_decoder`
  was retired in favour of `iso_oracle_samples` (same dual file, sha256
  `8039f2ad…9216`).

## iso-gain-map-metadata (CIPA DC-007 read — verdict)

**Status:** in progress
**Updated:** 2026-08-06

- 2026-08-06: **The CIPA documents are no longer blocked.** The "JavaScript/POST
  disclaimer gate that resisted scripted download" is just an undocumented POST
  contract: `std/js/dll.js` copies the page's query string into a hidden
  `dlltarget` field and posts it to `std/documents/dll.cgi`. So
  `curl -X POST .../dll.cgi --data-urlencode dlltarget=CIPA_DC-007-2025_E` returns
  the PDF directly; no browser needed. Same for `CIPA_DC-008-2026-E`. Both are
  free downloads gated only on accepting a no-warranty disclaimer. **Do not commit
  either PDF** — they were read from the scratchpad and only restated here.
- 2026-08-06: **First correction: DC-007 is Multi-Picture Format, not Exif.**
  DC-008 is Exif. Earlier entries here said "DC-007 ⇒ Exif-compliant", which is
  right only transitively — DC-007 §4.2.1 says a Baseline MP File *uses the
  compressed image file format of the Exif standard*, and §5.1 places the MP
  Extensions APP2 "immediately after the Exif Attributes in the APP1 marker
  segment". So the Exif requirement reaches us through MPF, and both documents are
  in scope.
- 2026-08-06: **DC-007-2025 explicitly anticipates us.** §4.2.1 names "gain map
  images specified by ISO 21496-1" as a recordable Dependent Image, and Table 4
  assigns them **MP Type Code `050000`**, with details to "follow the provisions of
  Annex C of ISO 21496-1". This is the interop contract nc is actually writing
  against, and it is far more on-point than the C.4.3 NOTE suggested.
- 2026-08-06: **Concrete non-conformance found, and it is small: nc's gain map is
  typed `Undefined`.** Measured on the dual-dialect file — MPImage1 is `030000`
  (Baseline MP Primary Image, correct) but MPImage2 is `000000`, which Table 4
  marks **× = "shall not be used"** in a Baseline MP file (.JPG). The correct value
  is `050000`. **This comes from libultrahdr, not from nc's code** — its own
  reference output (`ultrahdr_app` v1.4.0) writes `000000` too, unsurprisingly,
  since the gain-map type code postdates it. nc ships the file, so it is nc's gap.
  The repair is a 4-byte field in the MPEntry array, in the same post-packaging
  patch that already fixes the first image's recorded size. Note it would change
  the shipped `ultra-hdr-v1` bytes, which is why it is **not** being folded into
  the oracle fix.
- 2026-08-06: **Second gap, larger: the baseline is JFIF with no Exif APP1.** nc's
  header is `APP0 JFIF · APP1 XMP · APP2 ICC · APP2 ISO · APP2 MPF`, so the MP
  Extensions do not follow Exif Attributes as §5.1 specifies. Weighing how hard
  this binds: §7's tag-level requirement is "**should** be followed" for
  non-thumbnail Individual Images, and its tables are pinned to Exif 2.32 / DCF 2.0
  — so the *tag* obligations are recommendations. The structural statements in
  §4.2.1/§5.1 are the stronger ones. Adding Exif also changes the baseline's marker
  layout, which is precisely the class of change that just cost this task a
  silently inert feature, so it must be re-run against the ImageIO oracle rather
  than trusted to `cargo test`. Whether libultrahdr's `package()` preserves,
  rewrites or drops an APP1 Exif is **unknown and must be established by probe**
  (it has an `-x` Exif-insertion flag, so the native API may be the right route);
  the 2026-08-04 asymmetry lesson applies — do not reason it out from source.
- 2026-08-06: **Verdict: both items move to a follow-up task**
  (`output/mp-container-conformance`). Neither is required for the ISO metadata to
  function — Apple ImageIO reconstructs HDR from nc's file today with the type code
  `Undefined` and no Exif present — so they are conformance-claim work, not
  functional work, and they change shipped container bytes. Keeping them here would
  hold `output/presets` behind a container change that has nothing to do with the
  metadata this task owns. `baseline_carries_no_exif_colorspace_claim` stays as the
  tripwire; whoever adds Exif must still choose `Uncalibrated`, never `1`.

## ultrahdr-dependency-externalization

**Status:** not started
**Updated:** 2026-07-31

- 2026-07-31: Kept the reviewed local libultrahdr/libjpeg-turbo snapshot for the
  current gain-map change. Added this non-blocking follow-up to move dependency
  ownership back to Cargo after a published `ultrahdr-sys` release contains the
  required marker-order behavior and provides a fully pinned, network-free
  static native build. A Git dependency or system library is not the target
  because it would respectively retain repository-availability risk or make
  output depend on machine-installed native versions.

## ultrahdr-dependency-externalization (continued)

**Status:** not started
**Updated:** 2026-08-04

- 2026-08-04: Checked this task's trigger while working the ISO gate; it is
  **not yet met**, though half of it now is. Upstream libultrahdr released
  `v1.5.0` and `v1.5.1` on 2026-07-30, and `v1.5.0` **does contain** our pinned
  marker fix (`git compare` against `11ac0c32…`: 0 behind, 4 ahead). But this
  task's trigger is an exact published **Cargo** release, and crates.io
  `ultrahdr-sys` is still at **0.1.5 (2026-04-29)** — predating both the fix and
  those releases. So the snapshot, its force-tracked files, and
  `scripts/check-vendored-native.py` all stay until `ultrahdr-sys` publishes a
  version wrapping ≥ `v1.5.0` with a network-free static build. Re-check
  crates.io rather than upstream tags when revisiting.

## ultrahdr-dependency-externalization (re-scoped)

**Status:** not started
**Updated:** 2026-08-05

- 2026-08-05: **Superseding the preceding entry's trigger.** "Wait for a published
  crate wrapping ≥ v1.5.0" is not a sufficient condition and never was; inspecting
  the published archive (which the task's own How-to-Verify asked for) found a
  second, structural blocker no version bump can fix. Task **re-scoped** from
  "externalize the snapshot to a published crate" to **"remove the native
  dependency from the tree entirely."** The task **id is deliberately unchanged**
  — eight references depend on it, including `scripts/check-vendored-native.py:39`
  and this log's own append-only headings — so only the human-readable title moved.
- 2026-08-05: Evidence against the published crate. It is a third-party wrapper
  (`Enter-tainer/libultrahdr-rs`), **not Google's** — correcting an impression an
  earlier entry left. Still 0.1.5 (2026-04-29). Its bundled `jpegr.cpp` has no
  APP0 extraction (`grep -c "Extract APP0"` → 0), so adopting it would reintroduce
  the exact ordering that made ImageIO reject our files. The structural problem:
  libultrahdr's CMake takes libjpeg-turbo from
  `ExternalProject_Add(GIT_REPOSITORY … GIT_TAG 3.1.0)`. With the crate's
  `vendored` feature that is a build-time clone at a **mutable tag**; without it,
  `cargo:rustc-link-lib=jpeg` links a **machine-installed** library. First breaks
  pinning, second breaks the self-contained binary and makes output vary per user
  machine. The `GIT_TAG` sits inside the crate's own bundled CMake and
  `ExternalProject_Add` has no cache-variable override for it (unlike
  `FetchContent`'s `FETCHCONTENT_SOURCE_DIR_*`), so it cannot be pinned without
  forking — i.e. a local copy again.
- 2026-08-05: **What our snapshot actually is**, verified rather than assumed —
  worth recording because the obvious guess is wrong in both directions.
  `libultrahdr/lib/src/jpegr.cpp` is **verbatim upstream `11ac0c3`** (empty diff);
  I suspected local patches from its comment style and was wrong. But
  `libultrahdr/CMakeLists.txt` **is** modified: both libjpeg-turbo
  `GIT_REPOSITORY`/`GIT_TAG 3.1.0` blocks became `DOWNLOAD_COMMAND ""` so the build
  consumes the in-tree `third_party/turbojpeg` (pinned `20ade4de`). That two-line
  edit *is* the offline build, and it is exactly what no published crate provides.
  `patches/libultrahdr-no-threads.patch` is applied at build time on top.
- 2026-08-05: **The size motive does not survive measurement.** Whole-repo pack is
  **14.36 MiB**; vendor is 782 tracked files / 18 MB working tree. "Reduce
  repository size" was chasing a non-problem, which caps what the task should be
  willing to pay. The genuine cost is the maintenance apparatus: the
  force-tracking guard (needed because the copied upstream `.gitignore` hides
  legitimate files) and `check-vendored-native.py`.
- 2026-08-05: **Which readiness conditions are load-bearing**, after challenging
  both. *Static linkage: keep.* Linking a system libjpeg would lose the
  self-contained binary (a design-spec choice, and the same reason the HDR spike
  rejected HEIC/x265) and would make output vary by **user machine** — a bigger
  determinism hole than a build-time fetch, and one no test can see. Checking in
  prebuilt `.a`/binaries is worse on every axis: one artifact per target, larger
  than the source it replaces, unauditable, and only `aarch64-apple-darwin` is
  installed here so the Linux artifacts could be neither built nor verified
  locally. *No-network: too strict as written.* The property worth protecting is
  **pinning to an immutable revision**, not bundling; a SHA-pinned fetch would be
  fine. Relaxing it still does not unblock this crate, for the reason above.
- 2026-08-05: **The chosen route, and why the oracle is not kept as a
  dev-dependency.** Only **6** native calls are on the shipping path
  (`uhdr_create_encoder`, `uhdr_enc_set_compressed_image`,
  `uhdr_enc_set_gainmap_image`, `uhdr_encode`, `uhdr_get_encoded_stream`,
  `uhdr_release_encoder`) and they only write XMP + MPF around two JPEGs nc
  already encodes in pure Rust. **29 of the module's 46 `uhdr::` references are
  tests** — the decode-and-verify oracle. Keeping that as a dev-dependency was
  the first proposal and it was wrong: `cargo test` builds dev-dependencies, so
  CI would still need cmake/clang/nasm and the libjpeg fetch, and the dependency
  would have *moved rather than gone*. Its value is also narrower than it appears
  — libultrahdr reads only the **legacy** dialect, so it was never an ISO oracle,
  and the manual Apple/Android gate answers the same question with real consumer
  decoders. Replaced by captured goldens recorded **while the dependency is still
  present**, plus exiftool structural validation and the documented external gate.
- 2026-08-05: Two consequences to plan for. Assembling the container ourselves
  **retires `insert_baseline_iso_segment`** — with placement under our control both
  ISO segments go in directly instead of being spliced in after packaging — and it
  removes the marker-order bug class, since ordering becomes ours to state rather
  than inherited from libultrahdr's APP0-extraction fix. It also **changes the
  shipped `ultra-hdr-v1` bytes**, because our XMP will not serialize
  byte-identically. That preset is non-default and the gain map is not in
  `version::PIPELINE_FINGERPRINTS`, so no `pipeline_version` boundary is involved,
  but its determinism/golden assertions must be **re-captured deliberately, never
  adjusted until they pass**.
- 2026-08-05: The published-crate route stays recorded but unpursued, with real
  trigger conditions (contains `11ac0c3` **and** obtains libjpeg-turbo without a
  mutable-tag fetch or system library). Watching crates.io for a version bump is
  explicitly **not** the trigger. Our delta is small enough to upstream if anyone
  wants to try, but merge and release cadence would not be ours.

## hdr-avif-output
**Status:** done (2026-08-05, PR #78)

`hdr-pq` / `hdr-hlg` are live as explicit presets writing 10-bit 4:4:4 full-range
BT.2020 PQ/HLG AVIF. STEP 0 found the task's written design had no supply chain
(no published crate ships libavif ≥ 1.4.2; `libavif-sys` 0.17 is libavif 1.0.4,
predating `MA1A`; vendoring libaom is ~1,445 files / 45 MB), so with user approval:
**published `libaom-sys` 0.17.2 (vendors libaom 3.11.0, static, no network, no
in-repo snapshot) for the codestream + an nc-owned Rust MIAF/AVIF container writer
in `src/io/avif.rs`** (`avif-serialize` 0.8.9 hardcodes `compatible_brands:
[mif1, miaf]` with no setter). The writer's box layout was matched byte-for-byte
against a libavif 1.4.2 / aom 3.14.1 `avifenc` reference file (only the `iloc`
extent length differed, for codestream-size reasons); only `av1C` carries the
essential bit, and its `configOBUs` is empty. The whole file is built in memory and
committed through `io::staged`.

Facts `output/hdr-avif-windows-packaging` and anyone touching the encoder inherit:

- **Packaging.** `libaom-sys` is a plain `[dependencies]` entry with
  `default-features = false, features = ["av1_encoder"]`; the decoder is a
  `[dev-dependencies]` feature, and `aom_codec_av1_dx` is verified **absent from
  the release binary**. libaom's build needs cmake, a C/C++ toolchain, NASM (x86
  SIMD) and libclang for bindgen — the Linux/macOS CI jobs already install
  `cmake clang libclang-dev nasm` from the gain-map work. Only `aarch64-apple-darwin`
  is installed locally, so **the x86_64 Linux build was unproven until CI ran**
  (it passed). Windows is deferred by decision (CI matrix is
  `[ubuntu-latest, macos-15]`).
- **`av1C` must be parsed back out of the codestream.** `AV1E_GET_SEQ_LEVEL_IDX`
  reports the *target* level and returns **31** ("maximum parameters", not a
  level — real on a 74.6 MP scan); `parse_sequence_header` + `verify_codestream`
  read the truth back and refuse to package a file whose coded profile,
  still_picture, subsampling, bit depth, CICP, range or size disagrees with the
  renderer's contract; `level_name` renders 31 and the 24..=30 reserved range as
  names rather than "9.3".
- **libaom's packet list is per `aom_codec_encode` call.** Draining only after the
  flush silently yields a **0-byte codestream** (all-intra emits during the first
  call, `lag_in_frames` 0). Drain after every call.
- **`MA1A` is gated on the published AVIF v1.2 Advanced Profile limits**, quoted
  as named constants: High Profile, `seq_level_idx <= 16` (level 6.0; 17/18/19 are
  6.1–6.3 and over), ≤ 35,651,584 px, ≤ 16384 wide, ≤ 8704 high. Outside them the
  file is a valid general-brand AVIF and the report/`--strict`-promotable warning
  names the limit. **No grid path exists** — the spec permits either. The
  dimension gate is the **encoder's** `RANGE_CHECK` bound of 65,536 per axis
  (`av1_cx_iface.c:646-647`, a format limit), not `aom_img_alloc`'s `2^27`; over it
  is exit 4 before any allocation.
- **`clli` is measured** off the display-linear pixels (`dot(rgb, BT2020_LUMA) ·
  203`, MaxCLL = peak, MaxFALL = mean) and rides in
  `HdrRenderMetadata::content_light`; on `hdr-48bit.tif` it reads 114/41, not
  1000/203. HLG omits the box (display-referred).
- **Encoder settings are pinned parts of the preset, not knobs** (the
  `ultra_hdr::JPEG_QUALITY` precedent): `CQ_LEVEL = 8` (measured `cq` 0 / 8 / 12 /
  20 → 20.38 / 0.99 / 0.35 / 0.07 MiB at max code error 0 / 10 / 14 / 20 of 1023),
  one thread, no tiling, so repeated encodes on one build are byte-identical.
  `cq_level = 0` is mathematically lossless.
- **Codec bounds are pinned by equality, not tolerance**, because AV1
  reconstruction is normative and bit-exact. Measured with `avifdec`/dav1d at
  `cq_level` 8 on the four-class test field (max, RMS per plane): PQ
  `(9, 0.702) (10, 0.849) (9, 0.591)`, HLG `(8, 0.645) (8, 0.782) (7, 0.615)`. The
  committed test decodes with libaom and **reproduces those dav1d numbers
  exactly**, which is what lets an in-repo, CI-runnable decode stand in for
  `avifdec` (`decoded_code_error_stays_within_the_pinned_codec_bounds`). The
  cross-build contract is the weaker one: identical semantic metadata and decoded
  pixels within these bounds, never byte identity.
- Quantization clipping is **reachable, not defensive**: BT.2100-2 Table 9's
  full-range chroma row puts a saturated primary half a code outside the range, so
  those samples are counted into `EncodeReport`; a non-finite sample falls back to
  its own neutral level (0 luma, 512 chroma).
- `RunProfile::HdrAvif` calibrated across 18.66 MP and 74.65 MP scans: 78.47 B/px
  slope with ~7.9 MB fixed, so `AVIF_STAGING_BYTES_PER_PX = 48` leaves `accounted`
  3.4–3.8% under measured. A first pass at 64 B/px double-counted the allowance
  (1.43x measured). Its render phase is `sum(mul(image, 2)?, rendition)` — the
  shared source is `image`-shaped (carries IR). Peak phase is encode.
- The report's `avif` block carries facts read back out of the file; `avif.rendering`
  (2026-09-02) is the one deliberate exception, nested so declared policy is
  distinguishable from evidence. libaom's licence and the AOM Patent License 1.0
  are in `THIRD_PARTY_NOTICES.md`; counsel review of the patent grant stays with
  release.
- Verified end to end on the 18.66 MP Phoenix scan: 5184x3600, level 6.0
  (legitimately — 18.66 MP exceeds level 5.x's limit), PQ 1.03 MB, HLG 3.29 MB;
  `avifdec`, ExifTool and `sips` agree on brands, CICP, `clli` and depth.

## hdr-avif-windows-packaging

**Status:** not started
**Updated:** 2026-08-05

- 2026-08-05: Filed by `output/hdr-avif-output`, which shipped the AVIF encoder
  gated on macOS and Linux only. CI's matrix is `[ubuntu-latest, macos-15]` with no
  Windows runner, so the task's three-platform clause had no coverage and claiming
  it would have been false. This task adds a `windows-latest` job and proves the
  static libaom build under MSVC; no encoding behaviour changes. Note the contract
  it must *not* over-claim: byte identity is scoped per build/architecture
  (design-spec §8), so the Windows binary is not expected to reproduce the
  macOS/Linux bytes — only the semantic metadata and the pinned decoded-pixel
  bounds. If MSVC cannot build the vendored libaom source unpatched, prefer
  documenting Windows as unsupported over carrying a local patch; the repo already
  has one regretted native snapshot.

## lossless-hdr-tiff
**Status:** done (2026-08-06, PR #79)

Two chunks: **A** `hdr-linear-tiff` (bit-exact f32 display-linear BT.2020),
**B** `hdr-pq-tiff` / `hdr-hlg-tiff` (full-range 16-bit codes stored exactly + the
ICC `cicpTag` contract). Not gated on any paywalled standard — the 203/1000
numbers come from the closed spike, the signalling from ICC.1:2022, H.273,
BT.2100-3 and TIFF 6.0 (correcting `iso-gain-map-metadata`'s 2026-08-04 note that
grouped this task with the ISO 22028-5 purchase).

**Chunk A.** `hdr::LinearBt2020Hdr::into_parts` hands the buffer **by value** to
`io::encode::encode_hdr_linear` (a domain-typed entry point, so BT.2020 samples
cannot be confused with Rec.709 working images); the profile is
`color::hdr_linear_bt2020_icc` from `definitions::BT2020` at gamma 1.0, deliberately
with **no `cicpTag`** (H.273's full-range flag describes a bounded range these
samples exceed). Decoding the produced 18.66 MP file gives max exactly
`hdr::LINEAR_HEADROOM` = 4.9261084 with 7.92% of samples above reference white.
`RunProfile::HdrLinearTiff`: 18.66 MP accounted 820,917,504 vs measured
906,526,720; 74.65 MP 3,284,582,400 vs 3,578,101,760 — every term an enumerated
buffer, no free constant; its f32 `--export-ir` costs 0 (the plane is written from
its existing slice). The `tiff` writer streams strips under `Predictor::None`, so
there is **no container staging term**; a lossless-compression option would
reintroduce one.

**Chunk B — the profile decisions** (STEP 0 overturned the plan's own default):

- The PQ profile is an **extended-range A2B** (`lutAtoBType`, PCS `Y = L/203`,
  unclipped to ≈49.26), matching Adobe's reference `9-16-0-1
  BT2100-PQ-Display-Full.icc` (ICC v4.2, LUT-based, matrix = colorants × 0.5). A
  matrix-shaper TRC is confined to `[0, 1]`, so it could only clip at reference
  white or render everything at 2%. Built entirely through `lcms2-sys` in
  `color::synth_coded_hdr` (the safe crate cannot insert pipeline stages or expose
  the profile handle); no pixel passes through the unsafe region.
- **The HLG profile is scene-referred, and that is forced**: HLG's OOTF
  `R_D = α · Y_S^(γ−1) · R_S` is not per-channel separable, so no 1D curve set can
  carry it (Adobe's HLG Display profiles are ~66 KB 3D CLUTs). The PCS is anchored
  on `hdr::hlg_reference_white_signal()` (≈0.7499), computed from the renderer so
  the two cannot drift.
- Curve tables are **1024** entries (4096 gave identical accuracy; the limit is
  16-bit quantization of the stored values). Round trip through Little CMS: ≤0.1%
  above 20 nits, ≤0.8% above 5 nits, absolute error under 0.08 nits below.
- **The extended range survives only in float evaluation.** The `AToB0`'s output
  encoding is the same `u1Fixed15` PCS, so any **integer** ICC pipeline (including
  lcms's own 16-bit `cmsDoTransform`) clamps at ≈1.99997 ≈ 406 cd/m² — the same cap
  the `BToA0` carries. Documented, not engineered around.
- `MatrixCoefficients` in an RGB profile is **0** (ICC.1:2022 §10.3), never the
  AVIF path's 9. `convert_frame` dispatches on the preset **exhaustively**, because
  `hdr::transfer_for` answers for four presets and an `if let Some(transfer)` chain
  would hand the TIFF presets to the AVIF encoder.
- **Quantization scales in binary64.** An `f32` scale rounds twice: `0.996_498_05
  · 65535` lands on exactly `65305.5_f32` and stores 65306 where the nearest code is
  65305 — 271 of the 167,772 `f32` values in `[0.99, 1)` disagree, concentrated where
  PQ puts highlights. Out-of-domain samples are **rejected** with the pixel index,
  not clipped (the transfer stage guarantees finite `[0, 1]`). Measured RMS error
  0.286 codes on 18.66 MP, 0.2875 on 74.65 MP, against the `1/√12 = 0.2887` a uniform
  rounding residual predicts.
- **Cross-artifact verification:** decoding `hdr-pq-tiff`'s codes with an
  independent ST 2084 EOTF recovers `hdr-linear-tiff`'s samples to 0.0149% worst
  case over all 55,971,648 samples, with the above-reference-white count matching
  exactly (4,433,118 = 7.92%).
- **The sidecar carries the HDR contract**, not only the report: `--report none` is
  how a batch script runs, and the ICC provably cannot express reference white or
  peak. It rides in the sidecar's `meta` (the read side keeps `meta` as an ignored
  raw `Value`; a third sibling key would break `SidecarEnvelopeIn`'s
  `deny_unknown_fields`), as the same types the report serializes.
- The three HDR profiles carry real `profileDescriptionTag` values (`en_US`
  locale — a null locale showed no description in locale-requesting readers); the
  older sRGB/P3/ACEScg/ProPhoto profiles keep `"RGB built-in"` because renaming
  them changes shipped bytes.
- `RunProfile::HdrCodedTiff` measured on the 18.66 MP scan: accounted 820,917,504
  vs 906,346,496 (PQ) / 906,330,112 (HLG); render is the peak, the +6 B/px
  quantize buffer sits in encode. Every display profile shares one render term
  (`2·image + 12·px`), asserted by test.
- Two ICC conformance gaps (§8.4.2 `BToA0Tag`, §8.2 `chromaticAdaptationTag`) were
  deferred to `output/presets` and **closed there on 2026-08-09**.

**Viewer gate (2026-08-06): "valid and correct, but not discriminating."** A
Portra 400 review set (frames 1244/1249) across `hdr-pq-tiff`, `hdr-hlg-tiff`,
`hdr-pq` AVIF, `hdr-linear-tiff` and legacy sRGB: every file rendered correctly and
all looked alike. That proves ColorSync accepts the hand-authored A2B profiles; it
does **not** prove HDR presentation — the scene had diffuse rather than specular
highlights and the set used the then-default exponential curve. The documented
compatibility stays "limited-interoperability interchange, not display-ready". A
discriminating retest needs a specular-highlight frame and should compare PQ TIFF
against PQ AVIF.

Datapoint for `film-base/dmax-anchor-reliability`, recorded here because that review
produced it: Portra 400 frame 1229 (the fully-exposed reference) is clipped to zero
transmission in all three channels, so `estimate --d-max-region` correctly refuses it
and the review used `--d-max 1.35` (the median of measured rolls); and the film
**base does not transfer between capture sessions** as cleanly as the "0.0005
agreement" note implies — this roll's measured base (`0.5122/0.2270/0.1417`, frame
1230 with `--grid`) differs from the other Portra 400 roll's by **13% on green**,
enough to blow highlights when borrowed.

## mp-container-conformance

**Status:** not started
**Updated:** 2026-08-06

- Goal: make the gain-map JPEG a conformant CIPA DC-007 Baseline MP File — type
  the gain map `050000` instead of `Undefined`, and settle the Exif-baseline
  requirement (or narrow the claim, with the reason cited).
- Filed 2026-08-06 out of `iso-gain-map-metadata`'s DC-007 read; the findings and
  the reasoning behind the split are in that task's
  `## iso-gain-map-metadata (CIPA DC-007 read — verdict)` section above, and the
  approach is in [the task file](../tasks/output/mp-container-conformance.md).
- The one thing to carry forward before touching anything: a marker-layout change
  is exactly what silently disabled the ISO metadata (see the oracle section
  above), so **re-run the ImageIO decoder oracle** — the Rust suite provably
  cannot catch a placement regression on its own.
- 2026-08-06 (review pass on the oracle branch): that instruction is now
  followable — the Swift harness moved out of the scratchpad into
  `scripts/iso-decoder-oracle/` (macOS-only, not in CI, with a README covering
  build, sample generation, why `_EV` is required, and how to read `PRESENT` +
  headroom). Two supporting changes: `io::ultra_hdr` gained a container seam
  (`compress_images` + `package_images`) so the oracle's three files and the
  product go through **one** assembly path — a future Exif/MPEntry change cannot
  leave the oracle measuring a container nc no longer ships — and the three files
  now come from one render rather than three (`encode_with`'s output is
  byte-identical across the refactor, checked by sha256). Also recorded here
  because review surfaced it: in the **gain-map** image libultrahdr prepends its
  XMP, so `APP1` precedes `APP0 JFIF` — JFIF is not first in the dependent image.
  Pre-existing, harmless so far, filed as a third item on
  `output/mp-container-conformance`.
- 2026-08-06 (second review round): the seam went one level deeper —
  `io::ultra_hdr::assemble` now returns the container bytes and `package_images`
  is a thin `stage_bytes` wrapper, because the `dual_dialect_package` **test
  fixture** was still a second assembly path, and it is the fixture behind all
  four marker-order tests. Left as it was, an Exif APP1 added to the product
  would have moved the shipped layout while those tests stayed green — the same
  shape of hole this branch exists to close. The fixture now calls `assemble`;
  its bytes are unchanged (checked by a temporary equality test against the old
  inline construction), and
  `baseline_iso_segment_precedes_the_frame_header` was re-confirmed falsifiable
  through the new path by restoring the defective insertion point. Also retired
  `iso_sample_for_external_decoder`: `iso_oracle_samples` produces the same
  dual-dialect file (sha256 `8039f2ad…9216`, identical) plus the other two.
  Shipped `ultra-hdr-v1` still `67911f22…5540` on the Ektar frame.
- 2026-08-06 (third review round): **correction to how the oracle's result was
  reported above — the 4.926 headroom figure is not evidence of reconstruction.**
  ImageIO's `HDR decode: headroom 4.9261084` is `2^AlternateHeadroom`, i.e. nc's
  own declared `1000/203` policy constant parsed back out of the metadata.
  Measured both ways on 2026-08-06: the toy fixture with no `_EV`, whose gain map
  is flat (`GainMapMax = 0.000000` on all three channels), reports the *same*
  4.9261084 as the real frame at `+3 EV` (`GainMapMax = 1.095282`). So a pass
  condition of "PRESENT + headroom above 1.0" cannot fail on any file nc
  produces. What the oracle did establish is unchanged and still load-bearing:
  the **ABSENT → PRESENT flip** when the segment moved before `SOF0`, and
  ImageIO's field-by-field parse agreeing with what nc wrote. The discriminating
  number is `GainMapMax`; the harness README now states the criterion that way
  and names the echo explicitly, as do the task file, `TASKS.md`, and
  `insert_baseline_iso_segment`'s rustdoc.

## gain-map-dialect-activation

**Status:** not started
**Updated:** 2026-08-07

- Goal: verify the dual-dialect file on Android 15+, and give the ISO dialect a
  CLI path so a user can produce one.
- Filed 2026-08-07 out of `iso-gain-map-metadata`'s close-out; rationale and the
  `output/presets` boundary are in
  [the task file](../tasks/output/gain-map-dialect-activation.md).
- Two things to carry in: the sample set needs `NC_ISO_SAMPLE_EV=3.0` or the gain
  map is inert and the test discriminates nothing; and whichever of this task and
  `output/presets` ships a CLI surface first owns the `gain-map-hdr` name — the
  shipped `ultra-hdr-v1` is contractually ISO-free and must not be re-pointed.

## sdr-preset-followups

**Status:** not started
**Updated:** 2026-08-09

- Goal: hold the three open questions from the SDR presets — the default flip,
  Adobe RGB, and confirming the inherited memory profile — so the space stays
  visible rather than remembered. See
  [the task file](../tasks/output/sdr-preset-followups.md).
- Filed 2026-08-09 alongside `display-p3` / `compatibility`. Deliberately *not*
  answered: the user's steer was to track the work rather than lock the details.

## output-path-suffix

**Status:** not started
**Updated:** 2026-08-09

- Goal: `-o` names the output, the resolved preset supplies the container. An
  explicit suffix is still validated, and honoured verbatim when it matches.
- Origin: raised 2026-08-09 while updating `docs/using-nc.md` for the completed
  suffix table. Completing the table was right — `nc convert -o out.jpg` used to
  write a TIFF named `.jpg` — but it made the user responsible for knowing each
  preset's container, which is what the preset is for.
- The one thing to settle before writing code: `output/presets` states "the output
  path remains required and is never silently renamed" and owns container-aware
  roll naming. Completing an absent suffix is arguably not renaming, but that
  wording is the governing statement and presets is `[~]` in progress — agree the
  boundary with it rather than around it.

## sdr-preset-followups (default decided)

**Status:** not started
**Updated:** 2026-08-09

- 2026-08-09: **The default gamut question is answered: `display-p3`.** User
  decision, on the reasoning that nc's whole thrust is wide-gamut fidelity —
  which outweighs sRGB's "a default should surprise nobody" argument. Item 1 of
  the task file is now execution, not a choice.
- 2026-08-09: **Sequencing decided, and it changes what item 1 replaces.**
  `output/presets` proceeds unchanged and ships `gain-map-hdr` as the default
  first; real-image conversion, measurement and value tuning happen against that;
  the default then moves to `display-p3` here. So the incumbent this task
  displaces is `gain-map-hdr` (a JPEG), not `legacy` — the flip carries a
  container change as well as a pixel change, and the default path's tests churn
  twice across the two tasks. `output/presets` scope was deliberately **not**
  narrowed; the user chose the sequence over a scope change.

## presets
**Status:** done (2026-08-09, PRs #88 + #92)

Shipped in five chunks after the SDR pair: `display-p3` / `compatibility`
(2026-08-09, #88) → the dual-dialect `gain-map-hdr` preset → roll container-aware
naming → `custom` and the `--out-depth` replacement → the default flip
(`pipeline_version` 2 → 3) → the inherited coded-HDR ICC gaps. Planning history
(2026-07-21 … 07-30) settled `film-master` as the name for the unclamped linear
ACEScg branch, the atomic-preset rule, and `reference-anchored-sigmoid` /
`conversion-versioning` / `roll-conversion` as prerequisites.

**What shipped, and the rules that came with it:**

- **SDR pair.** `stages::render_sdr_preset` (display source → one `sdr::render` →
  `color::encode_rendered_sdr`) returning the same `Rendered` the legacy branch
  does; 16-bit TIFF, differing only in destination gamut. `RunProfile::SdrTiff` is
  **measured**, not inherited: 0.850 GB at 15.55 MP and 3.594 GB at 74.65 MP
  against estimates 0.921 / 3.911 GB, `accounted` 0.80x / 0.91x.
- **Suffix table and roll.** Completing `cli::required_extensions` (every preset,
  including `legacy` and `film-master`) closed the `-o out.jpg`-writes-a-TIFF hole,
  and briefly **broke `nc roll`** because `reject_roll_unsupported` derived
  "convert-only" from "pins a suffix". The convert-only refusal is gone entirely:
  `default_output_name` takes the frame's **own** resolved preset and
  `cli::derived_extension` (canonical `tiff`/`jpg`/`avif`; taking the head of
  `required_extensions` would rename every `_positive.tiff`, caught by an existing
  test), an explicit manifest `output` goes through `reject_suffix_mismatch`, and a
  test over `OutputPreset::ALL` pins that the derived spelling is a member of the
  accepted set. Extensionless output paths are **rejected as a decision** (design-spec
  §5), and the diagnostic names a preset only when the user typed one
  (`SuffixContext` carries flag presence). The two "accepted: …" lists in
  `OutputPreset::parse` are generated from `OutputPreset::ALL`.
- **`gain-map-hdr`** is `ultra-hdr-v1`'s render packaged with
  `Dialects::LegacyPlusIso`; the dialect rides in `FrameRender::UltraHdr`, both go
  through `encode_with`, and a test pins `output_stats` equal across the two. Verified
  with the ImageIO oracle on the CLI's own output (`PRESENT`, Display P3 base; the
  same frame as `ultra-hdr-v1`: `ABSENT`, headroom 1.0), also at 10368x7200 — which
  surfaced libultrahdr's compile-time `UHDR_MAX_DIMENSION = 8192` refusing real
  5000 dpi scans *after* the full render as exit 5; fixed by `ultrahdr-sys`'s
  `jpeg-max-dimension` feature (65500), no vendored source patched. The limit lives
  in a *decoder* helper (`jpegdecoderhelper.cpp`). `RunProfile::GainMapHdr` shares
  `UltraHdrV1`'s arm, measured on two sizes (18.66 MP estimate 1.095x measured, 74.65
  MP 1.060x, 90.6 → 88.5 B/px linear) and pinned as an **equality** so the ISO dialect
  cannot silently gain its own buffers.
- **`custom`** resolves the same legacy branch and the **same bytes** as the
  no-preset state; only provenance differs. It is the one non-atomic named preset,
  so atomicity is gated on `is_atomic()` (three call sites); `is_named()` is gone,
  since after the flip the default *is* a named preset. Widening `custom` to the
  modern display path is not attempted — the SDR renderer gamut-*maps* into a named
  space, so an arbitrary `--output-profile` has no destination there (the same gap
  `sdr-preset-followups` records for Adobe RGB).
- **`--output-hdr` / `--output-sdr` / `output.hdr` → one `OutDepth` enum**
  (`--out-depth u16|f32`, `output.depth`), migration errors, no aliases. `--out-depth`
  was the original spelling; PR #20 renamed it to `--output-hdr` before anything
  made "HDR" ambiguous — the premise expired, don't rename it back. **The
  presence-check exception survived the rename:** `--out-depth u16` resolves the
  documented default, so a value rule cannot see it while it still forces a depth an
  atomic preset cannot produce; `reject_out_depth_with_atomic_preset` keeps a
  flag-presence check. Telemetry's `conversion.output_hdr` became `output_depth`
  with `SCHEMA_VERSION` 3 → 4 (a renamed field is a wire change under any reading —
  this does **not** answer the enum-member policy question `sdr-preset-followups`
  holds), and `primary_depth_label()` (`u8`|`u10`|`u16`|`f32`) reports the primary
  container rather than the optional IR TIFF's depth.
- **The default flip** is v3 with its own `PIPELINE_FINGERPRINTS` row and
  [reports/render-defaults-v3.md](../reports/render-defaults-v3.md). **The drift gate
  cannot witness this bump**: `render`/`base` cover `reconstruct_and_print` and
  `film_base::estimate`, which the output preset does not select, so v3 carries the
  same two hashes as v2 and only `recipe` moved — the report, not the gate, is the
  evidence. Measured across seven rolls: no clipping on either version and a small
  consistent warming (red up to +0.017, blue down to −0.009, green flat). **The v2
  row is restored to `3d37b13ecb7a5095`** — the hash a v2 build actually emitted,
  with `"hdr": false`; once a bump lands in the same change, the prior row is history
  again and any in-place refresh must be undone (`drift_gate` recomputes only the
  current row). `tests/pipeline.rs`'s harness injects `--output-preset legacy` for a
  `convert` naming no preset, loading no `--params`, and writing `.tif`/`.tiff`
  (~87 tests about sidecars, staging and the memory model); tests about the
  *default* use `run_exact`.
- **The inherited coded-HDR ICC gaps closed:** `chromaticAdaptationTag` and
  `BToA0Tag` beside `A2B0`/`wtpt`/`cicp` (profile 6,708 → 31,516 bytes, stored codes
  untouched). Input class was evaluated and rejected: the two profiles differ in
  exactly 3 bytes, ColorSync treats them identically, so the decision fell to
  truthfulness (an Input profile describes a capture device). No decoder here
  enforces either tag — they are met on the normative text. The third fix rode along:
  `pinned::BT2020_TO_XYZ_D50` re-derived against `definitions::ICC_PCS_WHITE_XYZ`
  (ICC's declared `[0.9642, 1, 0.8249]`) instead of `D50.to_xyz()`, plus
  `XYZ_D50_TO_BT2020` and `BRADFORD_D65_TO_ICC_PCS`, each anchored on a relationship;
  `derive::rgb_to_xyz_adapted` is gone.
- **Behaviour bug found in review:** `--output-preset custom --linear-range …` exited
  0 and dropped the control, because the rejection was gated on the `Legacy` *name*;
  now gated on which **branch** renders (`Legacy | Custom`).
- **The default flip broke the committed real-scan harness in three places** with
  all four gates green (recipes omitting `output.preset`, `stage_freeze` still
  generating `output:{hdr:true}`, and the `*_positive.tiff` rename glob stranding
  JPEG outputs while printing success) — migrating checked-in artifacts is not the
  same as migrating what writes them. That became `analysis/harness-regression-tests`
  (done). `nctool compare`'s six fixture cases state `--output-preset legacy` so
  records stay comparable, which means **`compare` does not cover the product
  default** — recorded in `sdr-preset-followups`, since adding a case is
  `core/conversion-versioning`'s call.

**Handed on, in one place:**

- The default gain map is **inert** (1.0x) under the default sigmoid — see the epic
  summary. The "this frame's gain map is flat" warning for the dual-rendition presets
  is unwritten.
- `output/sdr-preset-followups` holds the next default flip (to `display-p3`, decided
  2026-08-09), Adobe RGB, the SDR report block (`RenderedSdr::metadata()`'s
  `#[allow(dead_code)]` is its marker; `stages::render_sdr_preset` drops
  `SdrRenderMetadata`), the luminance-only `sdr_range_warning`, the telemetry
  preset-enum policy, and the `compare` gap.
- `output/gain-map-dialect-activation`'s CLI half is consumed; Android 15+ remains.
- `cli.rs`'s rule-3 rustdoc lists 8 of the 9 display presets, omitting `gain-map-hdr`
  (pre-existing, prose only; the code keys on the branch).

## linear-render

**Status:** done (2026-09-01, PR #99)

Filed 2026-08-28 out of the `algo/exponential-anchor-placement` experiments. Both
display renderers applied a fixed Hermite shoulder that could not be switched off —
`shoulder_start` is `0.5 + 0.25/(1+highlight_compress)`, so hc=0 puts the knee at
0.75 and no value puts it later — while `algo::sigmoid` guarantees stage-3 output
**≤ 1.0** for `shoulder > 0`, so under the shipped default SDR was shouldered twice.
`highlight_compress` at 1 and 4 made every config *worse* (default 6.45% → 6.64%
blown; shoulder-less 21.38% → 21.58%) and rescued none; the Hermite's ceiling is
fixed at 1.0, so moving the knee only trades away in-range contrast.

**Shipped `print.display_tone` / `--display-tone <shoulder|none>`**, a selector
rather than a boolean or an "off" spelling of the width knob (user decision), on
**both** display branches (otherwise the HDR presets would accept a print knob and
silently ignore it). Measured on ten fixture frames under the shipped default
reconstruction: `blown%` fell on **all ten** (mean 6.5 → 4.9), `code sep` improved
on the three frames whose p90 sits above the knee and was blind on the rest,
midtones bit-identical (harness: `shadow_metrics::linear_render_probe`). The
residual ~4–5% blown is the *reconstruction's* — the sigmoid's own asymptotic
approach to 1.0 — which sized `output/display-tone-mapping`. Visual review passed
(user, 2026-09-01) toggling in place; that was the deciding check. `pipeline_version`
stays 3 — the default selector resolves to exactly the shoulder v3 applied — only the
`recipe` fingerprint refreshed (see `docs/progress/core.md`, 2026-09-01, for why
bumping would have been harmful).

Durable facts:

- **The Hermite *lifts* highlights** (concave, above the identity on `[0.75, 1]`), so
  removing it lowers and spreads them. That also explains the shipped inert gain map:
  SDR luma ends up ≥ HDR luma above the knee, so ratios are ≤ 1. Without a curve the
  two renditions agree exactly and the map is flat by construction — pinned by a test.
- **No curve-type gate.** The ≤1.0 guarantee is a property of `shoulder > 0` plus
  neutral print gains, not of the curve type; `sdr::render` already errors on any
  sample outside `[0, 1]`, so the mode polices itself. `--sigmoid-shoulder 0
  --display-tone none` exits 1 naming the pixel and the two ways out.
- **Two illegal states made unrepresentable:** the knee width rides *inside* the
  shouldered variant, and it is a `KneeWidth` newtype — an enum variant's fields are
  as public as the enum, and a bare `f32` payload let `highlight_compress = -1` render
  an infinite knee, i.e. a silent identity curve at exit 0 with metadata claiming
  `shoulder_start: inf`.
- **Diagnosis order:** the knee/tone contradiction rule runs *after*
  `validate_output_preset`, because on `legacy`/`custom` `highlight_compress` is the
  above-`1.0` soft clip and genuinely applies — running it first blamed the working
  knob.
- **The recipe encoding had room for a parameterized operator, measured not
  assumed:** unit variants serialize as bare strings under serde's externally-tagged
  form, so `{"reinhard": {…}}` is a pure addition and every stored recipe still
  parses; the cost is the CLI (`clap::ValueEnum` cannot derive over a payload).
- **The guard at diffuse white has zero margin by measurement**: film RGB `[1,1,1]`
  gives destination luminance exactly `1.0` on both gamuts, and Display P3's red
  channel is one ulp above 1.0, pulled back by the radial gamut map. A tolerance was
  rejected; a tripwire test pins it so a colorimetry re-pin fails in CI.
- **`output_render.display_tone` is a field, not prose.** `output_render.content` was
  derived from the preset alone and asserted "the reference-white-preserving shoulder
  … has run" under `--display-tone none`; tone is now a field carried by every preset
  (absent on `legacy`/`custom`/`film-master`, which have no display tone stage), and
  the `content` strings no longer name a curve. `--display-tone none
  --highlight-compress 0` and `--display-tone shoulder` on the non-display branches
  are accepted by design (identity values — the `--bigtiff auto` case).

## display-tone-mapping

**Status:** done (2026-09-02, PR #100; operator revised 2026-09-09 under #105)

Filed 2026-08-28 from the `algo/exponential-anchor-placement` tone-map probe. The
shipped Hermite knee reaches a fixed ceiling with zero slope, so content overshooting
by more than about a stop lands on it with zero separation (20.8% of a frame on SDR;
an HDR peak pinned at exactly 4.926); moving the knee only hurts, and knee-based
forms reserve only `1 − t` of output for everything above the knee (hyperbolic
`t = 0.85` left 27.7% blown where Reinhard left 6.1%). **Do not re-try a knee.**

**Shipped:** `--display-tone reinhard` / `print.display_tone = {"reinhard":
{"headroom_stops": …}}`, a third value of `linear-render`'s selector (a first
parallel `--tone-map` knob was discarded when #99 landed first), with
`--display-tone-headroom` defaulting to **6 stops**, display-referred (`W = 2^stops`;
user decision over density, which would have made a print key read the
reconstruction's anchor and contrast). `Headroom` is a checked newtype: a negative
headroom is not loud on its own — `2^-40` renders a solid white field at exit 0 with
the clip merely counted. Opt-in, no default moved, drift gate quiet. Operators live in
`pipeline/display_tone.rs`; `hdr.rs`'s copy of the Hermite is gone. `SdrToneMap` is one
enum resolved to a `ResolvedSdrToneMap` that both applies and reports; the enum's
serde tag carries the versioned operator name (one canonical name, no flat
`tone_curve` field).

**SDR evidence (2026-08-31, matched-midtone then matched-lightness probes).** At
matched midtone, extended Reinhard at `W = 64` beat the shipped sigmoid on both
`blown%` and `code sep` on all seven matchable fixture frames (E2/E3/G2/G3/P2/P3/P4;
G1/E1/P1 have no valid mid patch, and an earlier unmatched Gold "counter-example" was
an exposure artefact — **an unmatched comparison is evidence for a different
claim**). `W = 8` loses everywhere; `W = 256` wins but is effectively classic Reinhard
(pre-clamp peak 1.016, i.e. nothing left above diffuse white — the same condition that
makes today's gain map inert); **`W = 64` is the recommendation** (pre-clamp peak
1.26–1.30). Matching at mean encoded lightness instead of the mid patch showed the
"everything looks darker" verdict was the match point, not the operator: at equal
lightness `reinhard64` still wins on both metrics on 7/7, and the implied
mid-above-base offsets are 0.595–0.673 (mean **0.626**) — **not a calibrated
constant**; each row inherits its own sigmoid reference's variation, and a shipping
value needs `algo/sigmoid-parameter-calibration`'s bracketed roll and grey card. User
visual verdict on four frames: the tone mapping is preferred ("more details"); the eye
did not separate W16/W64/W256, so `W` cannot be chosen by preference.

**Colour cast: out of scope here, by user decision, and mis-measured once.** The
tone mapper is chromaticity-preserving by construction (it scales luminance and
applies one common factor per pixel). A percentile-bucketed channel-ratio probe over
scene content is **not a measurement of the film** and gave the wrong sign on Gold 200;
the clean measurement is on a target with no content (the leader — see
`film-base/dmax-per-channel-reduction`; since superseded by `algo/film-stock-profiles`).
The shoulder-less reconstruction *exposes* the per-channel model error the sigmoid's
shoulder was washing toward white; it does not cause it. Auto white balance could not
be evaluated because it resolves on the whole frame including the holder
(`algo/auto-anchor-interior-measurement`).

**HDR half (2026-09-02).** Both obvious ceiling-parameterized generalizations lift
mid-grey ≈14% and diffuse white ≈66%, so they fail the gain-map constraint (the two
renditions must agree below diffuse white). What works is
`g(v) = f(v) · (1 + (C − 1)·s(v))` with `s` a smoothstep in `log₂` from a crossover to
the white point — **the operator *is* the gain map**. Unbounded, its peak breaks the
declared 1000-nit ceiling (5.3–17.0 on seven frames); a hard clamp at the ceiling
collapses separation above reference white to **0.000 on four of seven** (one frame
alone read PASS — the single-frame trap again); so the base is **asymptotic**
(`v/(1+v)`, i.e. `extended_reinhard(v, ∞)`), giving peak 4.912–4.919 under 4.926 on all
seven with non-zero separation, at a cost of ≤0.0244% disagreement with the SDR base
below the crossover (a twenty-sixth of one 8-bit gain-map code step).
`hdr::render_linear` applies it with the crossover at **reference white**
(`REFERENCE_WHITE_CROSSOVER`), which is principled — below it both branches fit
inside SDR. `bounds_output()` had to **split** into `bounds_sdr_output` /
`bounds_hdr_output`: Reinhard is unbounded on SDR and bounded on HDR, and one boolean
asserted one of those wrongly. Separation above reference white is only 0.028–0.075
on five of the seven frames — reachable headroom barely used — and the lever is the
reconstruction's placement, not `W`. `hdr_gain_probe` builds each variant from its
parts and asserts the shipped one equals `highlight_lifted_reinhard`; a version that
derived variants by dividing through the shipped function collapsed all three onto it
silently when the base became asymptotic.

**The gain-map pair.** Admitted only after `gain_map::build` began ratioing against
`min(sdr, 1)` — the base as **stored**, which is what a decoder multiplies; ratioing
against the rendered SDR stored a gain short by whatever the encode clamped,
reconstructing up to 23% dark with every counter reading zero. The default
`gain-map-hdr` output stayed byte-identical. **`GainMapMax > 1.0` was never the
achievement**: `--sigmoid-shoulder 0` alone reaches 4.866x (98.8% of the ceiling, the
speculars fused into one plateau); the criterion is a conjunction and the separation
clause is what only an unbounded operator satisfies
(`the_unbounded_tone_separates_highlights_where_the_shoulder_plateaus`). On real
frames `GainMapMax` does not discriminate at all (4.8657x shouldered vs 4.7929x
unbounded, identical on every frame); the metric is the **plateau share** — the
fraction of the stored gain map pinned at its top code: 6.6–15.2% shouldered vs
0.26–0.61% unbounded, read off the stored map by `scripts/hdr-tone-review/`
(macOS-only, asset-dependent, its metric extractors fail hard rather than print
`nan`). **User verdict 2026-09-02 on the four-frame HDR review: shoulder-less
reconstruction + the unbounded tone preferred** over both the shipped default and
shoulder-less under the old knee — handed to `algo/reconstruction-render-curve-split`
(which reached its own verdict the same day; activation is
`algo/split-default-migration`).

**The diffuse-white cost.** As first shipped the operator mapped `W → 1.0`, which put
`1.0 → ~0.5`: 0.239 stops at middle grey and **1.000 stops at diffuse white** at every
`W` (the matched-midtone protocol hid it by matching where the cost is smallest). On
**2026-09-09** (`extended-reinhard-mid-preserving-v2`, under `algo/film-stock-profiles`)
the operator absorbed its midtone cost: an input gain solved so `f(0.18) = 0.18` at
every white point (`2m / ((1−m) + √((1−m)² + 4m/W²))`, rationalized — the textbook
root disagrees at `W = 2²⁴`). No member of this family can preserve mid-grey *and* map
`W → 1.0` (white-to-mid ratio floors at 6.17; pinning both needs 5.56), so the unity
point is `W / gain` and `f(W)` overshoots by 0.6% — on the one selector
`bounds_sdr_output()` already reports `false` for. Diffuse white now costs **≈0.86
stop** rather than 1.00, on **both** branches, at any headroom worth setting
(0 stops is the exact identity — `--display-tone-headroom 0` is byte-identical to
`none` on SDR *and* HDR, though the HDR form is a near-step curve at very small
headroom and does not approach the identity continuously). Whether that cost is
acceptable as a rendering intent is **open** and a default migration must decide it.
The encoded gain is unimodal, not monotonic to `W` (the roll-off past its peak is
under one 8-bit code step).

**Lessons the review rounds left, kept because they recur:** "right logic, wrong
place" (a rule written first in `validate_output_preset` handed `legacy`/`custom`/
`film-master` a remedy those branches refuse; value rules left only in the stage let
a 36-frame roll decode before failing — both halves of a rule belong in `validate`);
rebuilding onto a concurrently-merged PR silently dropped a regression test, nine unit
tests and a `#[serde(default)]`, with matching probe output as false evidence of a
faithful port; a bare-string `roll` overlay (`"reinhard"`) silently reset
`headroom_stops` to the default until `merge_json` learned that a bare tag naming the
base's own *struct* variant means "same variant, nothing stated"; and rustdoc prose
contradicting the code beneath it survives every gate — grep for the negation of the
claim after changing behaviour. All of these are in CLAUDE.md now.

## display-p3-default

**Status:** not started
**Updated:** 2026-09-13

- Goal: execute the 2026-08-09 decision that `display-p3` becomes the default output
  preset. Split out of `sdr-preset-followups`. Blocked in practice on deciding its order
  against `algo/split-default-migration`, which also moves the default and owes a bump.

## adobe-rgb-gamut

**Status:** not started
**Updated:** 2026-09-13

- Goal: Adobe RGB (1998) as a first-class, gamut-mapped SDR output. Split out of
  `sdr-preset-followups`; `definitions::ADOBE_RGB` already exists for the analysis tool.

## sdr-report-block

**Status:** not started
**Updated:** 2026-09-13

- Goal: a machine-readable SDR contract block in the report, the `hdr_coded_tiff`
  shape. Split out of `sdr-preset-followups`; `RenderedSdr::metadata()`'s
  `#[allow(dead_code)]` is the marker.

## sdr-jpeg-preset

**Status:** not started
**Updated:** 2026-09-13

- Goal: the SDR rendition as an 8-bit JPEG with no gain map. Filed 2026-09-13 when the
  user set the product shape (SDR lossless default; HDR lossless, SDR JPEG supported;
  HDR JPEG good to have) and this was the one of the four nc lacks.
