# Negative Converter — color Progress Log

Execution log for the `color` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status (the checkboxes);
this file is the narrative beside it.

One `##` section per task in this epic, named by the bare task name (the part
after the `/`). Read this whole file before starting a task in this epic, and
read other epics' `Epic summary` sections when you depend on them. Append
entries — don't rewrite earlier ones.

> **Consolidated 2026-09-13** (user-authorised; see CLAUDE.md's exception to the
> append-only rule). Sections of *done* tasks were rewritten as summaries keeping the
> decisions, gotchas and every measurement an open task cites; the full history is in
> git before that date. Sections of open and parked tasks are unchanged except: none.

## Epic summary

What other epics need to know about `color`:

- **The working space is linear Rec.709/sRGB primaries, D65, linear TRC.** That
  is a pinned assumption, not a measurement — decode gives scanner RGB with no
  input ICC, and `io/input-data-semantics` gates conversion to scanner-device +
  linear inputs so the assumption is at least honest.
- **NC film RGB v1 is the versioned boundary out of the film rendering.**
  `working_space::map_nc_film_rgb_v1(FilmRgbImage) -> AcesCgImage` applies one
  pinned 3×3 matrix (`AP1⁻¹ · Bradford(D65→ACES) · NPM_Rec709`) in binary64,
  stored `f32`, **unclamped**, IR carried through. The same mapper serves every
  reconstruction path — no per-curve fitted matrices. `v1` is frozen; a changed
  mapping is a new identifier under `core/conversion-versioning`.
- **This is film-rendering *intent*, not physical scene recovery.** The film,
  lens, development, and scanner rendering is deliberately preserved. Any measured
  neutralization is an explicitly selected correction
  (`color/optional-color-correction-profiles`), never a default or a prerequisite
  for P3, HDR, presets, or acceptance. Two earlier characterization tasks were
  closed as superseded for exactly this reason.
- **`AcesCgImage` is compiler-enforced.** Private fields, module-private
  constructor, and its only input is a `FilmRgbImage` — so a named colour output
  cannot be handed a value that skipped the mapper. Only *construction* is
  enforced: `io::encode` accepts any `LinearImage`, so the guarantee is that no
  one can *mint* an `AcesCgImage` outside the mapper, not that a film-RGB buffer
  cannot physically reach an encoder.
- **The mapper is total: it returns `AcesCgImage`, not `Result`.** Non-finite
  values pass through and are counted at encode.
- **Every named preset crosses the mapper; only `legacy` / `custom` do not.**
  `stages::render` dispatches on `output.preset`: `film-master` runs
  `reconstruct → map_nc_film_rgb_v1 → render_split::film_master` (no
  `color::to_output`); every display preset runs the mapper then
  `render_split`'s shared print controls then its renderer; the frozen
  `legacy`/`custom` path still runs `reconstruct → finish_print →
  color::to_output` and its pixels are unchanged (the golden fixtures are
  byte-for-byte green). The report stamps `working_mapping = "nc-film-rgb-v1"` on
  every path — provenance only, deliberately not a knob.
- **`to_output` does not clamp** and may hand the encoder out-of-`[0,1]` or
  non-finite values; clamping and loss counting belong to `io/encode`. It
  **consumes and returns** the image (in-place transform, `io/memory-preflight`).
- **`film-master` is the shipped unclamped-ACEScg branch** (`--output-preset
  film-master` / `output.preset`, `pipeline::render_split::film_master`, a pure
  unwrap that takes no `PrintParams`). It bypasses every print and display
  control and **rejects, loudly**: a frame-local `auto` Dmax *whose placement
  reads it* (`AnchorPlacement::reads_reference`; the reference-free anchors are
  cross-frame consistent and accepted), an actually-consulted `auto`
  `balance_range` (`density::consults_balance_range`), and every non-default
  print control — `print_exposure`, `black_point`, `white_balance`,
  `display_tone`, `highlight_compress`, `linear_range` — the sweep
  **destructures** `PrintParams` so a new control fails to compile there. `--out-depth
  f32` remains a *rendered* float TIFF on the legacy path and is never an alias
  for the master.
  **Its contract is the configured reconstruction, not a curve shape** (checked
  2026-09-02 by `algo/reconstruction-render-curve-split`): the branch already
  varies with `--density-curve` and every curve knob, and a shoulder-less
  reconstruction renders through it at exit 0. When `algo/split-default-migration`
  moves the default, the default master's *rendering* changes (unbounded above
  1.0, its documented contract); the branch and its report need no work.
- **Preset atomicity is checked on the *resolved value*, with exactly one
  presence exception.** A non-default `output.depth` / `output_profile` /
  `bigtiff` is rejected identically whether it came from a recipe or a flag, and
  a flag that resets a value to its documented default is accepted (that is how a
  graded roll recipe is re-exported as a master). The exception is the
  `--out-depth` **flag**, rejected by presence beside an atomic preset: `--out-depth
  u16` resolves the documented default so no value rule can see it, yet it
  *forces* a depth the preset cannot produce. (This began life as `--output-sdr`;
  `output/presets` renamed the pair to `--out-depth` / `output.depth` on
  2026-08-09 and the asymmetry survived the rename.) Gated on
  `OutputPreset::is_atomic()`, not `is_named()` — `custom` is named but not atomic.
  Don't generalize the exception, and don't remove it.
- **`nc roll` treats `output.preset` as roll-fixed**, alongside `film_base` and
  `reconstruction.curve.dmax`: a per-frame manifest override is applied but raises a
  loud, `--strict`-promotable roll warning, because it gives that frame a different
  *image class*, not just a different rendering.
- **The shared print controls (`render_split::display_source`) resolve
  `WB → exposure → black point → linear_range` once** and hand every display
  renderer the same `SharedDisplaySource`. `highlight_compress` and
  `display_tone` are deliberately *not* shared — they are per-renderer tone
  policy (`pipeline::display_tone`). A non-default `print.linear_range` or
  `print.display_tone` is accepted by every display preset and rejected on
  `legacy`/`custom` (rule 3 of `cli::validate_output_preset`, keyed on the
  *branch*, so a new display preset inherits acceptance).
- **Auto-WB domain shift.** The legacy estimator runs on pre-matrix film RGB;
  `resolve_shared_controls` runs the same estimators on mapped ACEScg, so an
  `auto` mode resolves to different numbers there — documented consequence, not a
  bug. `wb_channel_samples` filters only non-finite samples and
  `estimate_wb_gains` hard-errors on a channel *level* ≤ 0; post-matrix ACEScg
  legitimately contains negative samples, so a frame whose trimmed-mean/percentile
  level goes non-positive fails loudly rather than neutrally. No task owns that.
- **`pipeline/colorimetry/` is the single source of truth for every
  standards-based matrix and luma vector.** No stage may define its own: import
  from `colorimetry::pinned`. The runtime **never derives** — the binary64
  derivation and the audit harness are `#[cfg(test)]`. Product policy (reference
  white, peak nits, shoulder, gain-map offsets) still belongs to the stage that
  owns it, but must refer to a *named* space rather than restate its colorimetry.
  Changing anything there follows
  [`docs/colorimetry-maintenance.md`](../colorimetry-maintenance.md);
  `NC_COLORIMETRY_REGEN=1 cargo test colorimetry::audit` regenerates the audit
  artifact and **only** that — never `pinned.rs`. Four things downstream epics
  trip on: **(a)** two Bradford conventions coexist on purpose — the frozen
  `nc-film-rgb-v1` mapping needs Lindbloom's published inverse and new artifacts
  use the exact one; **(b)** the three luma vectors have three provenances and
  three verification rules — `BT2020_LUMA` is a normative table (deliberately
  *not* matching a derivation), `DISPLAY_P3_LUMA` an exact derivation,
  `SRGB_LUMA` the derivation rounded to six decimals (43 ulps, its own
  allowance); **(c)** the pinned-vs-derived tolerance is ±1 `f32` ulp, measured
  against the chromaticities' own three-decimal rounding moving entries ~3,500
  ulps; **(d) `pinned.rs` is not the only runtime consumer of a definition** —
  `pipeline::color` feeds `definitions::{REC709, DISPLAY_P3, ACESCG, PROPHOTO,
  BT2020}` straight into Little CMS (BT2020 joined with `hdr-linear-tiff`), so
  editing one of those **five** is a pixel change even with `pinned.rs` untouched
  and every audit ulp at 0, and nothing automated catches it (the drift gate stops
  before lcms2; the audit only compares pinned artifacts). A colour space the
  *analysis tool* needs is defined here first even when nc renders to nothing
  like it — `ADOBE_RGB` is the first (unused by the runtime), so
  `scripts/analysis/nctool/metrics.py` cannot become a second source of truth.
- **nc's ProPhoto output is a pure 1.8 power law** (`color::build_profile`
  omits the ROMM linear toe near black), so a consumer applying the specified
  piecewise curve disagrees with nc's own pixels below encoded 0.03125 — 1.3
  stops out at 0.01. Known, documented, unowned.
- **Note:** the log entries for `scanner-profile-before-density-experiment` are
  stranded at the tail of the `input-data-semantics` section in
  [`io.md`](io.md) — they lost their heading in the flat log before this epic
  split. Read them there.


## management
**Status:** done (shipped 2026-06-21, PR #7)

`pipeline/color.rs` over `lcms2` 6.1.1: `OutputSpace` (`SRgb` / `ProPhoto` /
`AcesCg` / `DisplayP3` since `output/display-p3-output` / `Custom(PathBuf)`),
`resolve_output_space(explicit, depth)`, `icc_profile(space)`, and `to_output`.
Decisions that still hold:

- **Working space = linear Rec.709/sRGB primaries, D65, linear TRC** — pinned,
  not measured (no input ICC in decode); synthesized as the transform's source
  profile. The `--input-profile` / `--assume-linear` knobs (`InputColor`) are
  parsed into config; any input→working conversion lives upstream in decode /
  orchestration, so this fixed working space holds.
- **f32 wide-gamut default = `AcesCg`** (AP1, ~D60, linear TRC; user chose it
  over ProPhoto/Rec.2020). u16 default = `SRgb`.
- **TRC is a property of the space, not the depth** — every embedded profile
  self-describes: sRGB curve (display), ProPhoto ROMM/D50 gamma 1.8 (display,
  modelled as **pure 1.8 with the ROMM toe omitted**), ACEScg linear (scene). So
  `--output-profile prophoto` is valid at any `--out-depth`.
- **This stage does not clamp**; range clamping and clipping warnings are
  `io::encode`'s. Intent `RelativeColorimetric`, `transform_in_place` on the
  interleaved `f32` buffer, IR untouched.
- `Custom` profiles must be RGB (CMYK/Lab/gray → `Usage`, exit 2); a misspelled
  keyword is a loud `Usage` error, never a deferred "cannot read ICC" path error;
  load/parse failures → exit 2, transform/serialize failures → `Other`.
- **lcms2 reports runtime transform faults only through the process-global
  `cmsSetLogErrorHandler`**, and `transform_in_place` is infallible. Resolved by
  `core/pipeline-orchestration`: `cli` installs the global handler via `lcms2-sys`
  FFI at startup (`AtomicBool` + stderr), `run_convert` clears it before the render
  and checks it after.


## post-reconstruction-color-characterization
**Status:** closed—superseded (2026-07-23)

Filed 2026-07-21 as an artifact-based characterization runtime (a versioned
`matrix3x3-with-input-curves` artifact mapping reconstructed film RGB into
ACEScg, with a Dmax-neutral `U = 10^(γ·D')` canonical input and a scalar
`10^(−γ·Dmax)` placement afterwards). Closed 2026-07-23 without implementation:
physical scene recovery is not NC's goal, so the film/lens/development/scanner
rendering is intentional by default and measured neutralization must be an
explicitly selected correction. Replaced by
`algo/negative-reconstruction-density-curves`, `color/film-rgb-working-space`,
`color/film-master-render-pipeline`, and `color/optional-color-correction-profiles`.
The task file keeps the rejected proposal as decision history. Its one durable
idea — a shared linear WB/exposure/black stage that SDR and HDR consume
identically — shipped as `render_split::display_source`.


## film-rgb-working-space
**Status:** done (shipped 2026-07-26, PR #54)

`src/pipeline/working_space.rs`: `map_nc_film_rgb_v1(FilmRgbImage) -> AcesCgImage`.

- **Pinned matrix** `NC_FILM_RGB_V1_TO_ACESCG` (f64), `AP1⁻¹ · Bradford(D65→ACES
  white) · NPM_Rec709`, Rec.709 primaries + D65 `(0.3127, 0.3290)` → AP1 + ACES
  white `(0.32168, 0.33767)`. Rows `[0.6130974, 0.3395231, 0.0473795]`,
  `[0.0701937, 0.9163540, 0.0134523]`, `[0.0206156, 0.1095697, 0.8698146]`; rows
  sum to 1; matches the published sRGB-linear→ACEScg Bradford matrix
  (colour-science / OCIO) to 5.2e-5, test-enforced against that external oracle.
  Applied per pixel in binary64, stored `f32`, unclamped, IR carried through.
  **It was pinned with Lindbloom's published 7-decimal Bradford inverse**, which is
  why `colorimetry` keeps `BRADFORD_PUBLISHED_INVERSE` beside the exact one
  (re-deriving with the exact inverse moves it 9.1e-8 — a pixel change to a frozen
  identifier).
- **Typed boundary:** `AcesCgImage` has private fields and a module-private
  constructor; its only input is a `FilmRgbImage` (itself only mintable by
  `algo::reconstruct`). No `trybuild` dev-dep — the privacy annotations are the
  guarantee, following the `algo/mod.rs` precedent. Accessors `width/height/rgb/ir`
  plus `pub(crate) into_linear`.
- **`working_mapping` report field** stamped `"nc-film-rgb-v1"`
  (`working_space::WORKING_MAPPING_ID`) on every convert — provenance only, not a
  flag or recipe key (§9 assigns it no home). A future mapping is a new
  identifier under `core/conversion-versioning`.
- **The mapper is total** (`-> AcesCgImage`, not `Result`); non-finite passes
  through and is counted at encode. Design-spec §7 was updated from the
  `Result` sketch to match.
- **For `color/optional-color-correction-profiles`:** a correction inserts
  *after* this mapper and *before* `render_split::{film_master, display_source}`;
  both consume `AcesCgImage` and inspect no provenance, so nothing in either
  module needs touching. The mapper stays uncorrected by construction.
- Verification is local to direct pinned Rec.709/D65 → ACEScg/D60 vectors
  (≤ 2e-6 vs binary64, multi-pixel to guard chunk boundaries); cross-encoding and
  decode-back belong to `analysis/display-output-acceptance`. Bit-identity is
  pinned per-pixel, never as a full-frame or post-lcms2 checksum.
- 2026-07-23: `docs/design-spec.html` retired as a maintained companion; the
  Markdown spec is the sole source.


## film-master-render-pipeline
**Status:** done (shipped 2026-07-28, PR #59, after three review rounds)

**What landed.** `src/pipeline/render_split.rs`, pure functions only:

- `film_master(AcesCgImage) -> LinearImage` is a **pure unwrap** and takes no
  `PrintParams` — that is the definition of the master, so any operation added
  there is a bug and no control can leak in by accident. `stages::render`'s
  `film-master` arm runs `reconstruct → map_nc_film_rgb_v1 → film_master` and
  **skips `color::to_output` entirely** (it would re-apply the Rec.709→ACEScg
  matrix), fetching only the ACEScg ICC blob. `ConvertReport::white_balance` stays
  `None` on the master by construction; the reconstruction's own
  `dmax`/`balance_range` *are* reported because they are part of what the master
  contains.
- `resolve_shared_controls(&AcesCgImage, &PrintParams) -> ResolvedPrintControls`
  resolves the shared controls **once** (an `auto` WB becomes concrete gains
  there), `apply_shared_controls` applies them in the pinned order
  `WB → exposure → black point → linear_range`, and `display_source` composes the
  two into one `SharedDisplaySource` owning exactly one `AdjustedAcesCgImage`
  (private fields, module-private constructor) — so SDR and HDR seeing the same
  adjusted source is structural, not a convention. `DisplayBranch` is the seam a
  renderer matches on. `highlight_compress` is deliberately not shared (branch
  tone policy; a test pins that a hot sample stays hot).
- `ResolvedPrintControls::new` is the sole, fallible, private-field constructor:
  finite positive WB gains, `exposure_gain.is_normal()` (a subnormal gain such as
  `2^-140` is finite and non-zero yet crushes every sample to a
  quantizes-to-black value with no counter firing), finite `black_point`, finite
  `low < high` with a positive representable span, **and per-channel
  `(wb[c] · exposure_gain).is_normal()`** — two individually-valid factors
  collapse to exactly `0.0` (`--white-balance 1e-30,… --print-exposure -100`).
  Deliberately *not* guarded, as inherent and loud via the non-finite counter:
  `px · gain` overflow for a validated gain, and a subnormal-but-positive span
  (`[0.0, 1e-40]` → `inf`).
- **Two knobs:** `--output-preset` / `output.preset` (one `OutputPreset` enum,
  never parallel bools; `legacy` *is* the no-preset state) and `--linear-range
  LOW,HIGH` / `print.linear_range` (default `[0,1]`, atomic pair, validated
  finite, `low < high`, representable span). `simple`'s `--invert-white-balance` /
  `--clip-low` / `--clip-high` are **removed and rejected** with a migration
  diagnostic naming `--white-balance R,G,B` / `--linear-range LOW,HIGH` and
  stating that the *value* carries over but the *pixels* do not
  (`wb_gains_do_not_commute_with_the_working_space_matrix`: non-uniform gains
  before vs after the matrix differ; a uniform gain commutes). Never promise
  bit-identical migration. (§7.1's "warned aliases at preset migration" were never
  activated; the display presets shipped without them.)
- **Report block `output_render`:** `preset`, `print_controls` (means "the stage
  ran at all" — `false` for the master *and* for legacy `simple`),
  `display_render`, `encoding`, `content`, `working_mapping`,
  `reconstruction_schema_version`. The master's `content` is **conditional on the
  resolved anchor** (`master_anchor`: roll-fixed Dmax / base-derived / no anchor
  / film curve) rather than a static claim, because validation accepts
  exponential `dmax = none`, `simple` has no anchor, and reference-free anchors
  read no Dmax. `pipeline_version` was deliberately absent until
  `core/conversion-versioning` stamped it.

**Rejections (all on the resolved config, so provenance does not matter; exit 2)
— today's form, see `cli::validate_film_master`:**

- **2a** frame-local `auto` Dmax, but only when the anchor placement
  `reads_reference()` — a base-derived placement discards the measurement and is
  cross-frame consistent (relaxed by `algo/exponential-anchor-placement`).
- **2b** an `auto` `balance_range` that is actually consulted
  (`density::consults_balance_range`, i.e. `shadow_balance != highlight_balance`);
  the default `Auto` range with equal balances is inert and stays accepted, which
  every default master depends on. Added in review: the master had accepted one
  frame-local measurement while rejecting its sibling
  (`--output-preset film-master --shadow-balance 0.1,0,0` exited 0).
- **2c** every non-default print control, named individually, via a
  **destructured** `PrintParams` sweep so a new control fails to compile.
- `scene-master` is rejected as an unreleased-schema break with "there is no
  alias".
- A flag that resets a recipe control to its default **is** accepted
  (`film_master_accepts_a_recipe_whose_controls_a_flag_resets_to_default`).

**Atomicity — the decision that reversed once, read this before "tidying".**
Round 1 unified every legacy selector on resolved-value semantics and deleted the
flag-presence rule, because presence and value checks had given *the same user
intent opposite outcomes depending only on provenance* (a recipe
`{"output":{"preset":"film-master"}}` plus `--output-sdr` exited 0 and wrote
2 783 902 bytes of f32; the flag preset plus `--output-sdr` exited 2). Round 2
**corrected** that for exactly one flag: `--output-sdr` (today `--out-depth`) is
rejected by *presence* beside an atomic preset, because its documented meaning
("force 16-bit integer output") is contradicted rather than subsumed — honouring
the preset silently discards an explicit container request — whereas `--bigtiff
auto` and a recipe `depth` at its default ask for nothing the preset does not do
and stay accepted. The three selectors (`depth` value / `output_profile` /
`bigtiff`) remain value-checked through the destructured
`OutputParams::non_default_legacy_selector`. The presence check needs the raw
flags, so it lives in **`validate_convert`**, which composes `validate` (config
only, shared verbatim by `roll` and every per-frame override) with the flag-presence
rule — **any new orchestrator must call `validate_convert`, not bare `validate`**.
The atomicity gate is `OutputPreset::is_atomic()` since `output/presets` added
the non-atomic `custom`; the `--bigtiff auto` acceptance means the size-based
classic/BigTIFF promotion stays delegated to `resolve_bigtiff` (a master over
~4 GiB legitimately comes out BigTIFF).

**`nc roll` warns on a per-frame `output.preset` override** (`sets_output_preset`,
a raw-JSON probe like `sets_curve_dmax`), `--strict`-promotable, applied not
rejected, even when the override restates the shared preset — a different
**image class** is the coarsest of the three roll-fixed breaks. The test uses
the IR-free `hdr-48bit.tif` plus a no-override control run: on `hdri-64bit.tif`
every frame already carries the "IR preserved but not used" warning, so a
`--strict` assertion there passes for the wrong reason (this became the repo's
strict-fixture convention).

**Telemetry:** `output_hdr` had read `cfg.output.hdr` (pinned `false` by the
preset while `depth()` returned `F32`); it now derives from `OutputParams::depth()`,
and `conversion.preset` was added — `SCHEMA_VERSION` 2 → 3.

**Legacy freeze pinned at the new boundary:**
`legacy_preset_render_is_the_frozen_reconstruct_print_colour_sequence` asserts
`render(…, default preset)` equals `color::to_output(reconstruct_and_print(…))`
bit-for-bit in-process — `stages::golden` calls `reconstruct_and_print` directly
and never crosses the preset `match`, so swapping the arms had left every golden
green. (`golden` pins **pre**-colour-transform pixels, not the legacy branch's.)

**For `color/optional-color-correction-profiles`:** insert between
`map_nc_film_rgb_v1` and `film_master` / `display_source`; nothing in
`render_split` inspects provenance. A corrected master must say so in
`output_render` — `content` is already conditional, so add a clause rather than a
second static string.

**Still open, recorded here and owned nowhere unless noted:**

- WB gains, `black_point` and `density.scale` are `positive()`/`finite()`-validated
  with **no upper bound**, while `sigmoid.contrast`/`toe`/`shoulder` and
  `linear_range`'s span are bounded. `algo/density-safety-bounds` owns the density
  half; the print half (the `exposure_gain` product guard above covers the display
  branches only) has no owner.
- `density::estimate_wb_gains` hard-errors on a channel level ≤ 0, and
  `wb_channel_samples` filters only non-finite; post-matrix ACEScg legitimately
  contains negatives, so auto WB after the boundary can refuse a frame the legacy
  estimator would accept. Unowned.
- Print controls under legacy `simple` are accepted and silently dropped
  (`finish_print` passes `simple` through; only the auto modes are rejected by
  `validate`), reported as `print_controls: false`. Needs a reject-or-warn
  decision; unowned.
- If `ResolvedPrintControls` ever gains `Serialize`, `serde_json` renders
  `f32::INFINITY` as `null`; the checked constructor prevents producing one today.
- The legacy `render_print` all-black underflow (`--print-exposure=-200` → 100 %
  zero samples, all loss counters 0, `--strict` rc 0; `--white-balance=1e-45,1,1`
  kills one channel the same way) is **handed to `algo/density-safety-bounds`**
  with its reproduction in that task file — a *different site* (stage-4
  `render_print`) from that task's original stage-3 context.
- Superseded: the exit-2-vs-exit-4 question for "not accepted yet" preset names
  (every name is accepted since `output/presets`; an unknown name is a typo).


## optional-color-correction-profiles
**Status:** optional / deferred
**Updated:** 2026-07-23

- 2026-07-23: Reframed measured scanner/film/development/lens neutralization as
  an opt-in CCR-like profile after the defined working-space boundary. Profiles
  must state what they correct and whether a lens is included.
- 2026-07-23: Kept capture, fitting, curves/matrices, and Delta E validation out
  of the default pipeline. This task depends on `film-rgb-working-space` and
  `film-master-render-pipeline` so it owns insertion before the split and
  corrected-master semantics; it has no downstream dependency edges.
- 2026-07-23: Pinned selection to `--correction-profile PATH` /
  `correction.profile.file` (default `null`), with correction immediately after
  NC film RGB v1 and before the film-master/display split. The optional task owns
  runtime integration, fail-loud artifact validation, hash/scope provenance, and
  corrected-master reporting.


## scanner-profile-before-density-experiment

**Status:** not started
**Updated:** —

- Goal: Determine empirically whether applying the same conventional scanner ICC transform to both image pixels and Dmin before component-wise density conversion improves negative reconstruction.
- **This task's two dated entries are stranded in [`io.md`](io.md)**, at the tail
  of its `## input-data-semantics` section: they lost their heading in the flat
  log before the epic split, so it carried them there verbatim. Read them before
  starting.


## colorimetry-source-of-truth

**Status:** done (shipped 2026-07-31, PR #67). Workflow: [`docs/colorimetry-maintenance.md`](../colorimetry-maintenance.md); step 7 of that doc appends dated entries **here**.

- Filed 2026-07-30 as accepted technical debt after the gain-map work added more
  standards-based matrices and luma vectors. `output/lossless-hdr-tiff` was made
  to depend on it so BT.2020 TIFF profiles reuse audited definitions.
- **Shipped as `src/pipeline/colorimetry/`**: `definitions.rs` (source data
  with provenance), `pinned.rs` (the literals the runtime multiplies by),
  `derive.rs` + `audit.rs` + `tests.rs` (**`#[cfg(test)]`**, which is what
  structurally guarantees "the runtime never derives"), and the generated
  `derived-artifacts.txt`. Consumers (`working_space`, `sdr`, `hdr`, `gain_map`)
  import from `colorimetry::pinned`; `color.rs` gained `lcms_inputs(ColorSpace)`
  so every Little CMS profile builder names a definition (this removed two
  drifting copies of the Display P3 primaries). PQ/HLG constants live in
  `definitions::transfer` (every PQ constant is a small-integer ratio, exact at
  both widths); the HLG OETF `a` and the sRGB type-4 TRC parameters followed
  (bit-identical narrowing verified: `0.178_832_77` is `3e371ff0` either way; the
  sRGB parameters are kept as the standard's quotients). Bit-identity verified
  same-machine on 21 stage renditions and five end-to-end outputs; the probe was
  deleted rather than committed (those paths run `powf`, so a checked-in bit-exact
  gate is red on the other CI target). `PIPELINE_FINGERPRINTS` untouched.
- **Phase 0 finding — the pinned f32 literals cannot all be reproduced exactly;
  check mode is ±1 ulp by measurement.** 33 of 36 matrix entries reproduce
  bit-exactly under every derivation variant (adjugate vs Gauss-Jordan,
  association order, four summation orders, Bradford vs CAT02); three
  (`ACESCG_TO_SRGB[2][1]`, `ACESCG_TO_DISPLAY_P3[2][0]`, `BT2020_TO_DISPLAY_P3[0][2]`)
  sit exactly one ulp off, and the derivation is one ulp **above** the literal
  (they read `+1` — an earlier raw-bit-subtraction `ulps_f32` had the sign
  inverted and overflowed `i32` on straddle-zero pairs; it now uses a monotonic
  `i64` key). Accumulation order moves the f64 result ~1e-17, seven orders short
  of the rounding boundary, so the residual is a source-data difference — most
  likely intermediate matrices rounded to ~9–10 digits in #61/#62/#63, whose
  derivation route was never committed. Not a correction: the standards'
  three-decimal chromaticities move entries ~4.2e-4, 3,544 ulps, so re-pinning
  would itself be the unreviewed pixel change the task forbids.
- **Two Bradford conventions coexist deliberately.** `NC_FILM_RGB_V1_TO_ACESCG`
  reproduces to 1.1e-16 with Lindbloom's published 7-decimal inverse cone matrix
  and is off 9.1e-8 with the exact inverse; the four display matrices are the
  reverse. `BRADFORD_PUBLISHED_INVERSE` exists solely so the frozen v1 mapping
  re-derives exactly; a test fails if the two are collapsed.
- **Three luma provenances, three rules.** `BT2020_LUMA = [0.2627, 0.6780,
  0.0593]` is the normative table (a derivation gives `[0.262700212, …]`, ~2e-6
  off — decoders invert the rounded form); `DISPLAY_P3_LUMA` reproduces the P3
  NPM Y row exactly; `SRGB_LUMA = [0.212_639, 0.715_169, 0.072_192]` is the
  derivation **rounded to six decimals** (0/−6/43 ulps) with its own
  `SRGB_LUMA_MAX_ULPS = 43` and a test pinning the 6-dp relationship — re-pinning
  it to the exact derivation is a pixel change on the sRGB SDR branch. The last
  two were found as inline array literals inside a `match` in `sdr.rs`, missed by
  two `const`-shaped greps (as was the HLG coefficient): **inventory a migration
  by reading the consuming functions, not by grepping for `const`.**
- **What the independence tests can and cannot see.** The recovered-chromaticity
  test anchors the **source** space only: the destination NPM appears in both the
  pinned matrix and the recovery step and cancels, so a mistyped *destination*
  primary is invisible to it. Display P3's only real external anchors are
  `color`'s two ICC-registry tests (`display_p3_colorants_match_icc_registry_reference`,
  `display_p3_decodes_to_registered_d65_encoding`); a realistic typo
  (`0.680 → 0.690`) is caught there, a sub-rounding one (`0.6805`) is caught by
  nothing but behaviour goldens, and that is not a defect (the standard does not
  define the value to that precision). `colorimetry/tests.rs` carries the
  per-space anchor table; expected chromaticities are re-typed literals, never
  the const under test.
- **The maintenance doc's "representation-only" rule was unsound as first
  written**: `pipeline::color` feeds `definitions::{REC709, DISPLAY_P3, ACESCG,
  PROPHOTO}` — and, since `hdr-linear-tiff`, `BT2020` — straight into Little CMS,
  so changing one of those five moves ICC bytes and lcms2-transformed pixels with
  `pinned.rs` untouched and every ulp at 0. Step 4 now warns; step 5 requires the
  before/after comparison for them.

- 2026-08-05 (added by `output/hdr-avif-output`, per step 7 of
  `docs/colorimetry-maintenance.md`): Added one new artifact,
  `pinned::BT2020_NCL_RGB_TO_YCBCR` — the BT.2020 non-constant-luminance
  R'G'B' → Y'CbCr matrix that AVIF signals as `matrix_coefficients = 9`. New
  `derive::ycbcr_from_luma` closes the standard's `Kr`/`Kb` formulas
  (BT.2020-2 § 3.4, BT.2100-2 Table 6) into a matrix; new
  `Source::YCbCrFromLuma` registers it in the audit catalog.
  **No existing artifact moved and every new entry audits at `ulps = 0`**, so
  this is an addition, not a pixel change: nothing shipped consumes it yet
  (`io::avif` is not CLI-reachable), no `pipeline_version` decision is owed, and
  none of the four Little-CMS-consumed spaces was touched.
- 2026-08-05: Two things about that artifact worth knowing before editing it.
  (1) It is the **first nonlinear-domain artifact** in the module — it multiplies
  transfer-encoded PQ/HLG code values, not linear light, which is exactly what
  "non-constant luminance" means. The file's other matrices are all linear-light
  transforms, so the usual "this is a colour transform" intuition does not carry
  over. (2) It is derived from the **tabulated** `BT2020_LUMA`, not from the
  BT.2020 primaries, and that is load-bearing rather than incidental: decoders
  invert the rounded tabulated form, so deriving from primaries would put nc's
  forward transform ~2e-6 away from every decoder's inverse. A test asserts row 0
  *is* the same pinned literal as `BT2020_LUMA`, so the two cannot desynchronize.
- 2026-08-05: Verification anchors follow the module's "oracle must not share a
  source" rule: the published four-decimal coefficients (as carried by ffmpeg /
  libavif / dav1d colour tables) at ±5e-5; exact `0.5` at full blue's Cb and full
  red's Cr, which is *why* the chroma rows carry the `2(1-Kb)` / `2(1-Kr)`
  scaling; a round trip through the matrix's own inverse; and an achromatic sweep
  over the **exact 10-bit code ladder**. That last bound is a measured maximum,
  not a round number — worst chroma residual 2^-25 at code 546, worst luma
  residual 2^-23. A first attempt swept 33 evenly-spaced values instead and
  understated the peak by 8x, because the residual is a rounding artifact that
  peaks near 0.5 rather than growing with the input.
