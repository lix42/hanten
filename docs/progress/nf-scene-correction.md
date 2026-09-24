# Hanten — nf-scene-correction Progress Log

Execution log for the `nf-scene-correction` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

Photographic corrections as a named stage: white balance, exposure, and the scene-referred half of the black point.

The epic was created on 2026-09-19 as part of the new-flow migration plan
(`docs/nf-migration.md`). **`stage`** (2026-09-22) filled `pipeline::scene_correction`:
white balance and exposure as one per-channel gain on linear ACEScg, recipe section
`scene_correction`, flags `--white-balance` / `--auto-wb` / `--exposure`, reported in
`new_flow.scene_correction` with provenance. What the remaining tasks build on:
`apply(image, &params, measure_region)` returns the resolved values beside the image,
so a new knob adds a field to `SceneCorrectionParams` (the recipe section), a
`recipe::merge` arm, a value rule in `check`, and a field in `SceneCorrection` for the
report. Auto white balance measures over the effective area, which makes it the
region's consumer on the new flow — a flare estimate over the same region would join
`measures_over_region`. Positive input enters ahead of this stage, at `AcesCgImage`.

**`roll-white-balance` (filed 2026-09-23) does not fit that recipe as it stands.** It is
a value measured once per roll from every frame, so `convert` — which sees one frame —
can only carry it, the way `calibration` carries a roll's base; where it is measured and
how it rides in the recipe are the task's first open question. It exists because
`nf-look/path-to-white`'s saturation band is placeable only behind it
([`docs/spike/desaturation-band.md`](../spike/desaturation-band.md)).

## stage

**Status:** done
**Updated:** 2026-09-22

- 2026-09-19: created with the new-flow plan. Goal: scene correction as a named stage.
- 2026-09-22: implemented after `nf-core/minimal-end-to-end` and `nf-core/recipe-schema`
  landed (the stage could not be reached, given a recipe key, or reported before
  them). Decisions, with the user:
  - **Spelling.** White balance keeps `--white-balance` / `--auto-wb` on both chains
    — the knob means the same thing, a per-channel gain after the 3×3. Exposure is
    renamed `--exposure`, since `--print-exposure` names a print stage the new chain
    does not have; each chain refuses the other's spelling. `flow::Availability`
    gained a `Renamed` verdict for that refusal. The recipe's `white_balance` takes
    only the tagged form (`WhiteBalance` is its own enum, not `WbSource`): the bare
    array is a compatibility alias the new recipe has no history to need.
  - **Auto white balance samples the effective area.** The whole-frame sample the
    current chain uses includes the holder and rebate. Consequence: on the new flow
    auto WB is the region's consumer (`SceneCorrectionParams::measures_over_region`),
    and because its gains reach every pixel, the "region measured" and "region reaches
    a pixel" predicates coincide there — `convert_frame` picks them per chain.
  - **Positive input enters at the working space**, before this stage.
- **Shape.** `scene_correction::apply(image, &params, measure_region)` resolves and
  applies, returning the resolved `SceneCorrection` beside the image; `chain::render`
  returns a `Rendered { image, applied, scene_correction }`, and the `applied` list
  moved there from `ChainParams`, since one stage's entry now depends on a
  per-frame estimate. The region is a render argument, not a param: it is a fact
  about the frame. The identity configuration skips the pass entirely.
- **Verification.** Unit: bit-exact identity, a hand-computed gain, order against the
  3×3 (the first runtime order test the chain has had), region-vs-frame estimate,
  estimate reuse, value refusals. Binary: each knob reaches the pixels and the
  report, the reported auto gains reproduce the image byte for byte, the region steers
  the estimate (`--measure-inset 0` differs), a roll per-frame
  `scene_correction.exposure` reaches only its frame, and an empty region refuses
  under `--auto-wb` with a remedy naming `--white-balance`. Mutation: sampling the
  whole frame instead of the region reds both region tests. The current chain's auto
  WB output is byte-identical before and after the estimator move (same machine).
- **Not done here:** no roll-consistency warning for a per-frame
  `scene_correction.white_balance` (an auto mode estimates per frame by design; a
  stated per-frame override is legitimate for exposure). That question belongs with
  the other new-chain per-frame overrides in `nf-core/subcommands`.
- 2026-09-22 (done): two review passes before the PR. Fixed: `applied()` now reads the
  folded gain, so an exposure too small to move `2^EV` off `1.0`, or a white balance
  the exposure cancels, reports `"identity"` — the same test `apply` uses to skip the
  pass; auto-WB failure and empty-region remedies name the recipe key, since `roll`
  takes no flags; `types::check_measure_inset`'s bound refusal, which is chain-blind,
  names both chains' region consumers (it said only `--auto-d-max`, which `--new-flow`
  refuses). Deferred: `flow::reject_new_flow_only_flags` is hand-coded for
  `--exposure` and nothing tests that direction — recorded in `nf-core`'s Epic summary
  for whoever adds the second new-flow-only flag. Rejected: the auto-WB sample's double
  copy (≤12 MB, capped, and the current chain's byte identity rests on that code).
  Unblocks `nf-scene-correction/{flare-removal,levels-knob}` and `nf-look/stage`.

## flare-removal

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: the scene-referred half of the black point.

## levels-knob

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: a home and a name for `linear_range`.

## roll-white-balance

**Status:** not started
**Updated:** 2026-09-23

- 2026-09-23: filed from `nf-look/desaturation-band-fit`
  ([`docs/spike/desaturation-band.md`](../spike/desaturation-band.md)). Goal: one white
  balance per roll, measured from the roll's own pooled top percentile (per channel,
  excluding pixels near the leader's density), removing the roll-constant cast and keeping
  the scene's light. `nf-look/path-to-white` depends on it: its saturation band cannot tell
  a cast white from skin without it, and a per-frame estimate removes sunsets first.
