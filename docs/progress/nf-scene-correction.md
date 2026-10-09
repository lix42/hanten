# Hanten — nf-scene-correction Progress Log

Execution log for the `nf-scene-correction` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

> **Since `nf-core/default-flip` (2026-09-27, `pipeline_version` 8)** the chain this summary calls the new flow is the only one: read "under `--new-flow`" as the default, and "the current chain" as the removed one (the reference build).

Photographic corrections as a named stage: white balance and exposure.

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
has no consumer inside a new-flow render.

**There is no scene-side black-point term (`flare-removal`, closed as not needed
2026-09-26).** Base fog is already in the measured film base, lens glare is part of the
photograph, and scanner veil is a highlight question. Placing black is fit range's
`--display-black`. So this stage produces no negative channels, and display black's
film-base reference has no scene subtraction to account for.

**`linear_range` retired, with no successor (`levels-knob`, 2026-09-30).** Its gain is
`--exposure` and its black is `--display-black`. Its refusals (`--linear-range`, the
`print` section, `simple`'s `--clip-low` / `--clip-high`) now name those two knobs. A
lifted black (a faded look) is the one thing it could do that nothing does now. If anyone
wants it, it is a look control.

**A per-roll midtone line runs first (`midtone-neutral`, 2026-10-08).** `measure-roll`
fits `roll.midtone_line` (10+ frames) and scene correction removes that cast before the
roll's gains, keyed on those gains, with a tint gate sparing strongly coloured light;
`roll.midtone_neutral` switches it per frame. Each frame's white is now measured after
the gains and the line, so anything tuned on whites (the thin lift, `display-white`)
reads corrected whites; the saturation check keeps the decoded white.

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

**Status:** closed — not needed
**Updated:** 2026-09-26

- 2026-09-19: created with the new-flow plan. Goal: the scene-referred half of the black point.
- 2026-09-25: pointer from [`nf-calibration/anchor-comparison`](../tasks/nf-calibration/anchor-comparison.md): its black probe showed the new chain needs a black
  point, which `nf-display-stages/parametric-operator` now owns (the film base as the
  reference). Scanner veil and base fog stay this task's, as a scene term. The two meet
  where the base renders, so measure fog against that level rather than a second one.
- 2026-09-26: **closed as not needed** (user, relayed from the
  `nf-display-stages/parametric-operator` session after display black landed in #175).
  The task's premise was a scene-referred additive term, "veiling glare and base fog",
  for this stage to subtract. None of the candidates is one:
  - **Base fog** is part of the unexposed film. The measured film base (from the
    rebate) includes it, and the decode references density to that base, so it is
    already removed.
  - **Camera lens glare** is light that reached the film: part of the photograph, not a
    correction toward the scene. If it is ever wanted, it is an opt-in look control,
    never a scene-correction default.
  - **Scanner veil** adds light in the transmission domain, where it matters most in
    the negative's dense areas (the scene's highlights). The clear base barely feels it.
    It is a scanner-calibration and highlight question, not a black one.

  Evidence: in `nf-calibration/anchor-comparison` every roll's darkest pixels sat at
  the film base itself (red p0.5 at −3.6 to −3.8 scene stops, base at −3.7), so there
  is no pedestal in the shadows. The legacy `--black-point` was therefore one job,
  placing black, which `--display-black` (`fit_range.display_black`) now does.
  Consequences: the per-channel grade's whole-pixel guard stays latent, since no stage
  upstream produces negatives; display black's reference needs no rule for a scene
  subtraction. Refusing `--black-point` under `--new-flow` as `Never` (pointing at
  `--display-black`), and removing the "flare half" wording from `fit_range`,
  `recipe`, the design docs and the guide, goes in `parametric-operator`'s follow-up,
  not here.

## levels-knob

**Status:** done
**Updated:** 2026-09-30

- 2026-09-19: created with the new-flow plan. Goal: a home and a name for `linear_range`.
- 2026-09-30: **retired (user).** The removed chain's `linear_range` was one scalar
  pair applied after the black point, `(x − low) / (high − low)`. Taken apart, every part
  but a lifted black is covered by another knob:
  - the gain `1 / (high − low)` is `--exposure`, `log2(1 / (high − low))` stops;
  - `low > 0` subtracts a black. `flare-removal` found no pedestal to remove, and
    display black places black by luminance rather than by a per-channel subtraction;
  - placing white with `high` is the look's contrast and the roll's white rule. A
    levels move there also shifts mid-grey, which the anchor is there to hold;
  - `low < 0` lifts black. Nothing does this, since display black never lifts. It is
    a creative look and not a correction, so it is noted here and not filed.

  The task's case for keeping it, a user with a measured range, does not hold: values
  at the fixed decode are not something a user measures outside the tool. Since
  `nf-core/default-flip` already refused the flag and the `print` section, the change
  is only wording: `--linear-range`, the `print` section's message, and `simple`'s
  `--clip-low` (→ `--display-black`) and `--clip-high` (→ `--exposure`), which pointed
  at this task too, now say what replaced them. The old default `0,1` is still refused,
  like the rest of the `print` section. Pixels don't change, so there is no fingerprint
  row.
- 2026-09-30 (done): verified through the binary. All four refusals (the three flags
  and a recipe `print.linear_range`) exit 2 and name their replacement, and
  `docs/using-nc.md`'s example matches the output word for word. Unit and pipeline
  tests check that each remedy is a visible `convert` flag and that no message
  mentions this task any more. Review: the diff reviewer found only a self-contradiction
  in this entry, now fixed; Codex was skipped because its workspace was out of credits.
  Nothing depends on this task.

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
- 2026-10-01 (cross-reference): the multi-frame memory growth recorded above is **fixed**
  by `io/multi-frame-memory-growth` — macOS malloc's cache of freed large blocks, not the
  per-frame model; `src/allocator.rs` maps big blocks directly. 35 Gold200 frames now
  peak at 0.60 GB (`measure-roll`) and 0.65 GB (`roll`). Numbers in `docs/progress/io.md`.

## midtone-neutral

**Status:** done
**Updated:** 2026-10-08

- 2026-10-07: filed (user) from the poor-development spike (`docs/spike/poor-development.md`;
  `../temp/roll-neutral-spike/TODO.md` B1, B2, detection switch). Five review rounds
  (`../temp/b3-review/` … `b7-review/`) chose the joined line in scene correction; no bad
  frame on two poor and two good rolls. The switch is a data floor (≥ 10 frames, enough
  voted bands), not a poor-development detector. The two-point slope it superseded is not
  filed. Fit range and fade width wait for ColorChecker frames.
- 2026-10-07: 1883's warm light gets a tint gate, not a turn-down (user, round 8,
  `../temp/b8-review/`; feasibility Artifact https://claude.ai/artifact/FKonxohqsff9nMmUkPMyf3).
  The correction fades as a pixel moves 0.6 → 0.9 (log2) from the line's cast; neutrals
  sit within ~0.23, 1883's underside at 0.73. Neutral patches unchanged (09-20 8.8, 09-29
  3.4); 1883 underside a\* 16.8 → 12.9 (today's WB 9.8); the user preferred the gate on
  1879, 1883 and 1886 and saw no difference elsewhere.
- 2026-10-08: **implemented** (`pipeline::midtone_neutral`; worktree branch). Decisions,
  with the user:
  - **Two-pass whites in this task**, mapped back to film RGB: each frame's film-RGB sample
    is taken to ACEScg, corrected (line, then gains) and mapped back through the 3×3's
    inverse, and the reviewed p97-of-the-brightest-channel rule runs there. The
    saturation check keeps the **decoded** white (`frames[].decoded_white_stops`):
    saturation is the film's. Whites moved, old → new: 09-28 +1.50 (floor) → +1.98, 09-14
    +1.82 → +1.93, 09-20 +1.68 → +1.79, 07-15 +1.95 → +1.76, 07-23 +1.79 → +1.63, 09-09 and
    09-29 within 0.02, the floor rolls unchanged. Short rolls move too: the gains alone
    correct them.
  - **Switch**: `measure-roll --midtone-neutral auto|on|off` (auto = the data floor); at
    render `--midtone-neutral on|off` / `roll.midtone_neutral`, line kept while off. A
    correction with a switch, an exception design-spec §6 now states (a beach roll can
    misread). `recipe::Lift` became `Switch`, since it now switches a correction.
  - **Roll-wide line** (`ROLL_WIDE`: a manifest that changes it warns), frame-local switch.
  - The line keys on the **roll's own gains**, so a stated `--white-balance` does not move
    it; a line without `roll.white_balance` is refused.
- 2026-10-08: **matching the spike**, three findings:
  - **The leader guard must not touch the line.** The spike's gains were guarded, its
    votes and fade end (`s_w`) were not. Guarded, 09-29's fade end sat 0.23 stop low and
    09-11 / 09-18 lost their top bands (red offset off by 0.05). So votes and fade end read
    an unguarded per-frame sample (the same one the whites use); only the gains are guarded.
  - **The band floor is a share** of the frame's sample (the spike's 300 of ~225,000, step
    8), not 300 pixels: at 2^17 samples a fixed 300 dropped sparse top bands.
  - Result: every coefficient within 0.006 of the spike's on the seven rolls ≥ 10 frames,
    identical band ranges, and no line on the 3- and 4-frame rolls. Rendering, the spike's
    hook and `--roll-midtone-line` differ by ≤ 1 u16 (09-29 2033, 09-20 1883, 09-18 1774);
    the branch-measured line against the spike's moves pixels ≤ 257 u16 (p99 ≤ 42), against
    an effect of up to 10,572. `MIN_BANDS` 3 is not reviewed: every reviewed roll counted 9+.
- 2026-10-08: **drift gate refreshed in place, no row** (the task file asked for a row): the
  two keys are `null` by default and no earlier recipe can state them, the precedent of
  three earlier `roll` keys. A recipe from `measure-roll` does render differently now
  (whites, line), as `level-target-zero`'s did without a bump. Memory: `measure-roll` holds
  two samples per frame (~110 MB at 36 frames; `pipeline::memory`), and frees the pool
  after the gains. Golden: `golden_midtone_neutral_is_correct_within_its_libm_window`
  enumerates ±1 ULP on each of the six `f32` libm calls (729 combinations per pixel).
  Pending: the moved-whites review (`../temp/whites-review/`).
- 2026-10-08: **moved whites accepted** (user, `../temp/whites-review/`: 09-28 Portra 400
  dark and 09-14 Ektar, whole rolls, SDR Display P3; main / line with main's whites / line
  with the new whites). The new whites read a little dark; the user attributes that to
  diffuse white at L\* 79, which `nf-calibration/display-white` raises, not to the
  measurement.
- 2026-10-08: **memory figure corrected**: after the frame loop `measure-roll` holds three
  samples per frame (film RGB, its ACEScg copy, the pooled white's copy), ~160 MB at 36
  frames (`pipeline::memory`), not the two (~110 MB) stated above.
- 2026-10-08: **done.** Landed as one change: `pipeline::midtone_neutral` (line, gate,
  measurement), applied first in scene correction in the same pass as the gains;
  `measure-roll` writes `roll.midtone_line` and measures whites after the correction.
  Review: Codex, `nc-reviewer`, the user's `/code-review` and ship's reviewer; fixes
  included a non-finite pixel left alone (it turned NaN), the line held flat below its
  lowest band, `measure-roll --midtone-neutral off` skipping the measurement (`bands: []`),
  and the docs saying render-time off keeps whites measured with the line (re-measure with
  `off` for that; a later `--params` layer's `null` cannot clear a line). Accepted as is: a
  pixel with a channel ≤ 0 after the gains gets the full correction (the spike's rule, not
  reviewed on saturated colour). For dependents: `display-white` and the thin-lift
  thresholds now read whites after the correction; `midtone-neutral-fit` owns the fit
  range and fade width; the bands are in pre-exposure stops, so a thin roll's rendered
  shadows can fall below the lowest band (the spike's design, unreviewed on thin rolls).

## midtone-neutral-fit

**Status:** not started
**Updated:** 2026-10-07

- 2026-10-07: filed (user) from the poor-development spike (`docs/spike/poor-development.md`, `TODO.md` round 5 and
  E1): fit range and fade width could not be settled by eye or by patch medians; the
  ColorChecker rolls (Gold200, Ektar100 shot) carry picture frames, so each roll's line
  can be checked against its chart.

## correction-confidence

**Status:** not started
**Updated:** 2026-10-07

- 2026-10-07: filed (user) from the sea probe (`../temp/beach-probe/`, Artifact
  https://claude.ai/artifact/FEa5gytdqhFfUnKvVhjzKa): frames the user marked as sea pull
  the roll white balance and the midtone line warm (09-14: white balance blue −405 against
  a random-draw max of 190, patch cast 110 → 465). The user's direction: corrections stay
  on by default; warn when in doubt, turn off only when sure, and score each in the
  report. `--rendering direct` is not yet a safe fallback (patch casts 340–500).
