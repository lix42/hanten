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
Landed so far: **`legacy-custom`** (2026-09-23), **`sigmoid-and-simple`** (2026-09-23),
**`display-tones`** (2026-09-24).

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

**What `sigmoid-and-simple` means for the rest of the epic.**

- **The current chain's default is the fixed decode's configuration** — the exponential
  at contrast 2.0 and `mid-at-base-offset(0.62)`, read from `algo::fixed`'s constants
  (`pipeline_version` 6). Both chains render the same default reconstruction, so
  `default-flip` no longer moves the decode, only the stages after it.
- **The default reads no `Dmax`.** A stated reference is carried and warned about
  (`unconsumed_dmax_warning`, keyed on `render_reads_the_reference`). What still reads one
  is `--anchor-white-at-reference` / `--anchor-mid-fraction` — `dmax-machinery`'s whole
  remaining surface. The report's `reconstruction_result.curve.dmax` still resolves
  `fixed` / 1.3 under the default placement even though nothing read it; that is left
  for `dmax-machinery` to remove rather than re-shaped here.
- **No reconstruction is bounded at white any more**, so `--display-tone none` refuses
  ordinary content on SDR; tests pull the fixture down with `--print-exposure`. That
  sharpens `display-tones`' case rather than blocking it. The default gain map is live.
- **`Reconstruction` is a struct** (`density` + tagged `curve`); `characteristic` is the
  only other curve, so `DensityCurve` has two members and `ConversionPreset` three — both
  `characteristic`'s to finish.

**What `display-tones` means for the rest of the epic.**

- **The display tone's one knob already lives at its final key**, `fit_range.headroom_stops`,
  on both chains (`ResolvedConfig::fit_range` is `recipe::FitRange`). `print-prefix-rename`
  has no display-tone key left to move — `print` is now exposure, black point, white
  balance and `linear_range` only.
- **`ConversionPreset` sets three knobs** (curve, `density.scale`, `print_exposure`), not
  the tone; the presets' old reinhard-at-6 is simply the default.
- **The current chain's SDR tone is fit range's operator bit-for-bit; its HDR form is not**
  (asymptotic base, strictly under the peak). `default-flip` retires `pipeline::sdr`/`hdr`
  and with them that difference. Zero headroom reports `"identity"` on both chains.
- **Old sidecars carry `"display_tone": "shoulder"` and are refused**, the old default
  included (it replays differently) — so a pre-v7 sidecar needs that key deleted before it
  replays. `highlight_compress: 0` is stripped.

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
- 2026-09-23: **ship review** (diff-reviewer + Codex; Codex found nothing). The
  retired-flag errors told a `--new-flow` user to pick a preset, which that chain
  refuses; the remedy is now flow-aware ("drop it"), and the new-flow test asserts the
  preset advice is absent. The golden module's "two of twelve are not reference
  captures" became four of ten.

## display-tones

**Status:** done
**Updated:** 2026-09-24

- 2026-09-19: created with the new-flow plan. Goal: retire the `shoulder` and `none` tones.
- 2026-09-24: **plan.** Decisions taken with the user before starting:
  1. **`pipeline_version` 7.** The default tone moves from `shoulder` to reinhard at 6
     stops, which moves every default display render; the drift gate stops before the
     display stages, so the `render` hash is unchanged and only `recipe` moves.
  2. **No selector survives.** `fit-range` settled reinhard as the one operator, so
     `--display-tone` is a removed flag at every value (`reinhard` included — it is
     always applied), and `print.display_tone` is a migration error at every value
     (`shoulder` is the old default but replays differently; carrying a `reinhard`
     headroom across would be an alias). The headroom moves to `fit_range.headroom_stops`
     in `ResolvedConfig` — the new recipe's key, so neither `print-prefix-rename` nor
     `default-flip` renames it again. `--highlight-compress` is removed; the recipe key
     is stripped at its old default `0` and refused otherwise. `KneeWidth` goes;
     `Headroom` stays (it is the white point). Legacy HDR keeps its asymptotic-base
     `highlight_lifted_reinhard` until the flip — only SDR is fit range's operator
     bit-for-bit.
  3. **The over-range refusal survives on zero headroom.** `--display-tone-headroom 0` is
     the identity, and it now refuses an over-range sample on SDR as `none` did (HDR
     already did). A non-zero headroom keeps counting the SDR overshoot at the encode.
  4. **Report fields** `shoulder_start` / `highlight_compress` and `NO_TONE_CURVE` go.
- 2026-09-24: **implemented; all gates green, not yet reviewed.**
  - `DisplayToneCurve`, `KneeWidth`, the Hermite shoulders, `tone_curve_id` and the
    `bounds_*` predicates are gone; the renderers take a `Headroom`, and
    `Headroom::is_identity(crossover)` keys the zero-headroom refusal on both branches.
    `accepts_reinhard_tone` folded into `applies_display_tone`, and
    `validate_output_preset`'s rule 2 went with it.
  - **No presence rule for the headroom.** `film-master`'s value sweep catches a
    non-default headroom from either provenance, and a presence rule would have refused
    the flags-win reset (`--display-tone-headroom 6` over a recipe's value).
  - Conversion presets no longer own the tone (their expansion was reinhard at the
    default, which is now simply the default), so a recipe's headroom survives `--preset`.
  - The roll overlay guard `names_the_same_externally_tagged_variant` existed only for the
    reinhard struct variant and is deleted, as is the tone-switch headroom warning.
  - **A latent gain-map defect, found because this made reinhard the default.**
    `encode_legacy_gain_map` ratioed its luminance gain against the *rendered* SDR, not the
    stored `min(sdr, 1)` that `gain_pixel` uses. One far-over-white sample (a scan value of
    0 in `ultra_hdr_v1_native_reconstruction_covers_odd_dimensions_and_hdr_vectors`)
    drove `GainMapMin` to log2 −42 and the libultrahdr decode to garbage (white → PQ 0)
    at exit 0. Reproduced on `main` with `--display-tone reinhard`; fixed and pinned by
    `the_legacy_luminance_gain_ratios_against_the_stored_base` (mutation-checked).
  - Default `GainMapMax` on `hdr-48bit.tif` (`--film-base 0.9,0.55,0.42`): 1.88 → 0.93
    log2. **`--strict` at defaults can now fail** where it passed: the shoulder never
    clipped, while reinhard's SDR overshoot beyond the headroom is counted and warned
    (neither fixture clips at defaults; documented in `using-nc.md` §7). The task file's
    "`Headroom` leaves with `highlight_compress`" was wrong — `Headroom` is the white
    point and stays.
- 2026-09-24: **code review** (`/code-review`, 10 findings, all taken). Zero headroom now
  reports `"identity"` as the operator (report and both renderers' `tone_curve`), the new
  chain's rule via `fit_range::IDENTITY`; `Headroom` resolves the white point and gain once
  per frame instead of per pixel, and `convert_frame` resolves it once; the two chains'
  headroom refusals share `types::headroom_fault_message`; the zero-headroom errors name
  `fit_range.headroom_stops` beside the flag, since `roll` takes no flags;
  `Recipe::to_config` carries `fit_range`; three stale comments fixed.
- 2026-09-24: **ship review** (`ship:diff-reviewer` + Codex; Codex found nothing). Taken:
  CLAUDE.md's default `GainMapMax` (1.88 → 0.93); the `reinhard` recipe migration no
  longer claims a lost render (moving the headroom renders identically — only `shoulder`
  and `none` point at the reference build); the `none` remedy is scoped to display
  presets; `design-update.md`'s "Today" column and several stale sentences. **Declined:**
  stripping `"shoulder"` from `film-master` sidecars (it replays identically there). The
  recipe is loaded before the final preset is known — a flag can change it — so the strip
  would have to be preset-aware at load; the refusal is loud and its remedy is one key.
- 2026-09-24: **done.** Verified: all CI gates green (fmt, clippy `-D warnings`, build,
  801 unit + 236 integration tests after rebasing onto #155/#156, 392 `nctool` tests),
  `cargo doc` with no warnings, and `docs/using-nc.md` re-verified against the binary. The task's three checks
  hold: removed names get removed-value errors on flag and recipe, on both chains
  (`the_retired_display_tone_flags_are_refused_with_a_migration_error`, unit and binary);
  no rule, field or message names a retired tone except as history; zero headroom refuses
  over-range on SDR and renders it on HDR
  (`the_zero_headroom_ceiling_is_per_branch_not_one_reference_white`).

## sigmoid-and-simple

**Status:** done
**Updated:** 2026-09-24

- 2026-09-19: created with the new-flow plan. Goal: retire the sigmoid and `simple`.
- 2026-09-23: **plan.** Decisions taken with the user before starting: (1) the current
  chain's default moves off the sigmoid to the exponential at the fixed decode's
  configuration (`gamma` 2.0, `mid-at-base-offset(0.62)`, scale `[1, 0.84, 0.73]`), and
  `ExponentialParams::default` moves with it — a `pipeline_version` 6 bump; (2)
  `Reconstruction` collapses into a struct now, the wire's `"type": "density"` accepted at
  its old default and `"simple"` refused; (3) `sigmoid-knees` / `sigmoid-flat` become
  plain removed-value errors, and `scripts/sigmoid-baseline/` is deleted. Tests that used
  `simple` as a cheap fixture move to `FilmRgbImage::fixture`; sigmoid goldens and probes
  are deleted, not re-pointed.
- 2026-09-23: **done.**
  - **Default (v6).** `ExponentialParams::default` reads `algo::fixed::{CONTRAST,
    MID_ABOVE_BASE}` and `default_scale_for(Exponential)` returns `fixed::DENSITY_SCALE`;
    `fixed`'s `the_current_chains_default_is_this_decode` pins `Reconstruction::default()`
    to its equivalent configuration. New `PIPELINE_FINGERPRINTS` row (render
    `752e701021a41307`, recipe `dbac245a916032f2`, base unchanged); v5's behaviour frozen
    as a literal. `golden_new_default` recaptured. Telemetry schema 5 (`reconstruction`
    dropped, `curve` always present).
  - **Removed:** `algo/{sigmoid,simple}.rs`, `SigmoidParams`, `REFERENCE_SHOULDER`,
    `ReconstructionType`, `AnchorPlacement`'s `Default`, the sigmoid validate rules, the
    `simple` merge/validate arms (`--auto-wb` under `simple`, preset-over-`simple`),
    `active_density_domain_flag`, the `sigmoid-knees` rules
    (`brightness_is_in_the_anchor`), and `UnpinnedCurve::AnchorOnly`. Migration errors:
    `REMOVED_SIGMOID_CURVE` (recipe + a custom `--density-curve` parser),
    `REMOVED_SIMPLE_RECONSTRUCTION` (recipe + hidden `--reconstruction`), hidden
    `--sigmoid-*` flags naming `--density-gamma` / `--display-tone` / `--anchor-*`, and the
    two presets by name. `reconstruction.type` is accepted only at `"density"`.
  - **Tests:** the eleven module fixtures use `FilmRgbImage::fixture`; `all_configs`
    loops cover exponential + characteristic. Deleted: the two sigmoid goldens, the
    `simple` golden, `the_linear_rendered_sigmoid_takes_its_brightness_from_the_anchor`,
    `each_candidate_look_needs_its_own_print_exposure` (without the sigmoid looks the
    remaining three start within 0.125 stop of each other, under its 0.25 bar — the
    per-look exposures now rest on 1.59–1.91 across the characteristic presets), and ten
    `shadow_metrics` probes that rendered a sigmoid (`tone_map_review.html` with them).
    `curve_probe::sigmoid_scale` stays: it measures density slopes and renders no curve.
    In `tests/pipeline.rs`, `simple` fixtures were rebuilt on a stated exponential and
    `--display-tone none` tests pull the fixture down 2.2–5 stops.
  - **Found on the way:** with an unread default reference, the HDR SDR-range warning and
    both over-range errors advised levers that no longer worked (`--d-max`, "bound the
    reconstruction"); they now name `--print-exposure` / `--anchor-mid-offset`.
    `explicit_dmax_domain_warning` would have fired on every default `--d-max` run; it and
    `unconsumed_dmax_warning` now share `render_reads_the_reference`, so exactly one fires.
  - **Scripts:** `scripts/sigmoid-baseline/` deleted except `fixtures.json`, moved to
    `scripts/analysis/fixtures.json` (nctool's default and three docs read it).
    `scripts/hdr-tone-review/` deleted too — beyond the agreed plan: its page is the
    sigmoid study's measured findings, which a config swap would have left wrong.
    Preset matrix, `benchmark.json` and nctool's roll freeze (default curve
    `exponential`) updated.
  - **Asset probes:** the `#[ignore]`d `curve_probe` / `shadow_metrics` set panics before
    measuring on today's `../nc-assets`, whose rolls were renamed (`Ektar` →
    `2026-07-15-Ektar100`); unrelated to this change, and not fixed here.
- 2026-09-24: **review round** (`/code-review`). All ten findings taken. The ones that
  change behaviour: the legacy-recipe migration error no longer advises the refused
  `reconstruction.type = "simple"`; the HDR SDR-range warning names only
  `--print-exposure` (the `--anchor-*` family is refused under `characteristic`); and
  **`nctool roll convert` makes Dmax opt-in** (`--measure-dmax`, user decision) — it
  froze a leader Dmax the default placement never reads, so every frame warned and
  `--strict-roll` failed the roll. `--d-max` still freezes a stated value; the leader
  frame is required only when measuring. Also: `REMOVED_SIGMOID_CURVE` no longer claims
  the default sits "at the same anchor" (it names `--anchor-mid-fraction 0.5` as the old
  placement); the film-master negative-sample guarantee is back, as
  `io::encode`'s `the_film_master_branch_writes_a_negative_sample_unclamped` (fixture →
  mapper → split → f32 bytes); every "does the render read `Dmax`" question now goes
  through `render_reads_the_reference`; and stale `simple`/sigmoid prose went from
  CLAUDE.md, `stages.rs`, `gain_map.rs`, `recipe.rs` and the design-spec example.
- 2026-09-24: pre-ship review (Codex + local reviewer). The removed-sigmoid-flag
  remedies fire on both chains (`reject_removed_flags` runs before the flow table), so
  each now also names what works under `--new-flow` (`--anchor-mid-offset`; display
  tone "not yet available"), with a test that the named new-flow flag is accepted.
  `nctool roll convert` refuses `--dmax-region` without `--measure-dmax`, and drops a
  partial recipe's retired `reconstruction.type = "density"` before merging — `hanten
  params` no longer writes the tag, so the merge read it as a variant switch and
  replaced the whole default reconstruction. Stale prose fixed: `--no-d-max` is unity
  placement only under `--anchor-white-at-reference`; `--auto-d-max` *is* warned about
  under the base-derived anchors; telemetry heading is schema 5; comments citing
  deleted tests went. All gates green.

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
