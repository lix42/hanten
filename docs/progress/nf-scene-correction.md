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
`scene_correction`, flags `--white-balance` / `--exposure`, reported in
`new_flow.scene_correction`. What the remaining tasks build on: `apply(image, &params)`
returns the resolved values beside the image, so a new knob adds a field to
`SceneCorrectionParams` (the recipe section), a `recipe::merge` arm, a value rule in
`check`, and a field in `SceneCorrection` for the report. Positive input enters ahead
of this stage, at `AcesCgImage`.

**White balance is measured per roll, never per frame (`roll-white-balance`, done
2026-09-23).** `hanten measure-roll` pools every picture frame's effective area at the
fixed decode's output, guards against a fully exposed frame with the leader, and
reports gains that are then *stated* here — so the stage estimates nothing and the
render stays per-frame pure. The per-frame `gray-world` / `percentile` modes and
`--auto-wb` retired on this chain (refused by name). The measurement region therefore
has no consumer inside a new-flow render; a flare estimate over it
(`flare-removal`) would be the first, and would reintroduce the region argument and
the empty-region refusal the stage no longer needs.

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
**Updated:** 2026-09-25

- 2026-09-19: created with the new-flow plan. Goal: the scene-referred half of the black point.
- 2026-09-25: pointer from [`nf-calibration/anchor-comparison`](../tasks/nf-calibration/anchor-comparison.md): its black probe showed the new chain needs a black
  point, which `nf-display-stages/parametric-operator` now owns (the film base as the
  reference). Scanner veil and base fog stay this task's, as a scene term. The two meet
  where the base renders, so measure fog against that level rather than a second one.

## levels-knob

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: a home and a name for `linear_range`.

## roll-white-balance

**Status:** done
**Updated:** 2026-09-23

- 2026-09-23: filed from `nf-look/desaturation-band-fit`
  ([`docs/spike/desaturation-band.md`](../spike/desaturation-band.md)). Goal: one white
  balance per roll, measured from the roll's own pooled top percentile (per channel,
  excluding pixels near the leader's density), removing the roll-constant cast and keeping
  the scene's light. `nf-look/path-to-white` depends on it: its saturation band cannot tell
  a cast white from skin without it, and a per-frame estimate removes sunsets first.
- 2026-09-23: **started; three decisions (user).** (1) A **roll-measuring command** writes
  explicit gains into the recipe — rendering stays per frame, `convert` reproduces from
  the recipe alone; a `roll` pre-pass was rejected for that reason. (2) **Per-frame auto
  white balance retires** on the new chain. (3) **Gains only** — the roll white's level,
  which `gamma-split`'s candidates C/D would also read, waits for a consumer; share the
  measurement then, not a parameter.

  Retirement review (`nf-retire/*`): only `dmax-machinery` and `print-prefix-rename`
  touch this. The leader-`Dmax` family retires and is **not** reused — the guard here
  reads the leader fresh, as `dmax-machinery` already requires of any content white's
  guard. The effective area and `measure.inset` stay, and this becomes their consumer.
  `print.white_balance` is replaced by `scene_correction.white_balance`, which carries the
  measured gains unchanged. `density.offset` stays out of it: the decode is fixed and
  stock-agnostic.

  **Estimator study** at the fixed decode (contrast 2.0, anchor 0.62 above base), four
  rolls cached in `../temp/roll-wb/` (`scripts/study.py`, `study.json`):

  | estimator | in-range W clean/ramp/kept | C kept/ramp/pulled | blown frame (worst roll) | sunset drop |
  |---|---|---|---|---|
  | p97, no guard | 4/5/3 | 14/2/1 | 1.25 st | 0.011 st |
  | p97, guard 0.1 | 5/2/5 | 15/2/0 | 0.000 st | 0.010 st |
  | **p99, guard 0.1** | **8/3/1** | **13/3/1** | **0.000 st** | **0.004 st** |
  | p99.5, guard 0.1 | 7/4/1 | 9/7/1 | 0.001 st | 0.004 st |

  **p99 with a 0.1-density leader guard** matches the gains the marked whites ask for —
  Gold200 `[1.00, 1, 1.28]` vs `[1.03, 1, 1.30]`, Ektar `[1.20, 1, 1.20]` vs
  `[1.20, 1, 1.21]` — and its misses are the band fit's ambiguous patches, which no white
  balance fixes. **The guard is load-bearing:** the leader added as a frame moves the gains
  0.4–1.3 stops without it (Gold200 0.40 … 09-11-Portra400 1.28), 0.000 with it. Dropping any single frame moves Gold200 and Ektar ≤ 0.055
  stops, but 09-11-Portra400 (11 frames) up to 0.2.
- 2026-09-23: **landed.** `pipeline::roll_white` (pure: leader guard, per-frame pooling
  capped at 2^17 px so every frame weighs alike, pooled per-channel p99, green-anchored
  gains) behind a new `hanten measure-roll` command, which decodes each frame at the
  fixed decode into ACEScg — the point scene correction's gains apply — and samples its
  effective area. It requires an explicit film base and reports the gains as a
  `--white-balance` flag and a `scene_correction` recipe fragment (typed, so the
  fragment prints the flag's `f32` digits). On Gold200 it gives `[1.002, 1, 1.277]`
  against the study's `[1.00, 1, 1.28]`; with the leader mixed in as a frame the gains
  are **identical** guarded and move 0.37 stops unguarded.

  Per-frame auto white balance is gone from the new chain: `WhiteBalance` keeps only
  `Explicit` (the tagged spelling stays, being the recipe contract), `--auto-wb` is a
  `Never` row naming `measure-roll`, `recipe::check_body` names the retired modes, and
  the report's `provenance`/`estimator`/`region` fields went with them (the
  regional-balance precedent: drop a field that became constant). The chain lost its
  `measure_region` argument; the effective area is still resolved and reported on
  every run. Goldens for the two auto modes were deleted, not re-pointed.

  **Memory:** `RunProfile::MeasureRoll` is the new flow's render phase with no encode,
  calibrated on two frame sizes (+22.0% at 16.43 MP, +19.7% at 18.66 MP; enumerated
  buffers 0.87x of measured). **A multi-frame run outgrows any per-frame model** —
  35 Gold200 frames peaked at 2.78 GB against 0.61 GB for one — because frames differ
  by a few pixels and the allocator cannot reuse a freed buffer for a slightly larger
  one (the same frame five times stays flat). `roll --new-flow` grows the same way
  (0.70 → 1.30 GB over five frames): a pre-existing gap in the gate, recorded in
  `memory.rs`'s calibration notes, not fixed here.

- 2026-09-24: **review round (`/code-review`), fixed.** `measure-roll` copies
  `convert_frame`'s new-flow front half and had drifted from it: it dropped the
  effective-area warnings (a capped holder march pooled holder strips into the white,
  silently, even under `--strict`) and checked the inset only inside the first frame's
  decode, blaming that file. Both fixed, and both sites now carry a comment that a gate
  added to one belongs in the other — a shared helper would have to untangle
  `convert_frame`'s report and IR notes, which is more than this task. Also: `--strict`
  without `--leader` and a frame named twice are refused before any decode (exit 2); the
  recipe's `scene_correction` is reset before validation, since the command measures it
  rather than reads it, and its messages name keys only; the retired-mode migration
  message now says "drop it, then state the gains `measure-roll` reports", which works
  from `measure-roll` too instead of looping. One bad frame still aborts the run — a
  calibration that quietly drops a frame changes the roll's white — and the leader still
  decodes whole (~0.15 s; cropping first would need a second decode path).
- 2026-09-24: **ship review (Codex + diff reviewer), fixed.** (1) Frame weighting: the
  integer-stride sampler halved a frame's sample just past a multiple of the cap
  (131,073 px kept 65,537), so frames did not weigh alike; `sample_region` now takes
  exactly `min(pixels, cap)` evenly spaced pixels. Gold200 moved to `[1.0021, 1, 1.2774]`
  (≤ 0.0005), every frame now contributes exactly 131,072, and the leader-as-frame
  result still holds (identical guarded, 0.37 stops unguarded). (2) The unparsable-ICC
  warning `convert` gives was one more gate the copied front half missed; added. (3) The
  `--leader` file among the inputs is refused, since its unguarded edges would pool.
  (4) The retired-mode migration message fires only on `gray-world` / `percentile`; any
  other string is left to serde's "unknown variant".
