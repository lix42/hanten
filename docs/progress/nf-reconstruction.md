# Hanten — nf-reconstruction Progress Log

Execution log for the `nf-reconstruction` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

The fixed, stock-agnostic decode: exponential, one anchor rule with a frozen `d`, `gamma` split into a calibration half and a look half.

The fixed decode has landed (`src/algo/fixed.rs`, 2026-09-22): fresh arithmetic,
bit-identical to the equivalent legacy configuration, reading no reference density,
with the reconstruction half of the knob-availability inventory beside it. It is not
yet reachable from the CLI — `nf-core/minimal-end-to-end` wires it to a destination.

**Three things other epics need from it.** The decode's parameters are
`algo::fixed::DecodeParams`, **not** the resolved `reconstruction` object: under
`--new-flow` a recipe stating that section is refused whole, which is blunt and
temporary until `nf-core/recipe-schema` gives the new stages a spelling, and which
means `roll --new-flow` can state no decode knob today. The surviving knobs
(`--density-scale`, `--density-offset`, `--density-gamma`, `--anchor-mid-offset`) are
accepted but nothing maps them yet — `nf-core/minimal-end-to-end`'s, along with a
`RunProfile` that must be *measured*, since the fused decode holds one buffer fewer
than the legacy staged path. And the remaining `--new-flow` remedy defects in
`cli.rs` belong to `nf-core/knob-availability-audit`.
**One spike is also done and its result is an input to other epics**. `anchor-spike` costed four ways to place the decode's white
([`docs/spike/white-placement.md`](../spike/white-placement.md)) and found that under
today's proposed anchor all three rolls measured land **0.55–1.28 stops short of white**,
so a per-channel highlight operator has nothing to act on. Anyone building on this epic,
and `nf-calibration/anchor-comparison` in particular, needs that before they start.

## fixed-decode

**Status:** done
**Updated:** 2026-09-22

- 2026-09-19: created with the new-flow plan. Goal: the fixed, stock-agnostic decode.
- 2026-09-22: **planned — four decisions taken before any code, plus two forced by the
  tree.** The task file is not the record: these are here.

  **(1) A fresh module with its own parameter type — and it has to live inside
  `src/algo/`.** The migration rule's strong form was chosen over reusing
  `Reconstruction` with flow-conditional defaults: `algo/fixed.rs` owns `DecodeParams`
  (`scale`, `offset`, `contrast`, `anchor`) and writes the arithmetic fresh. *Where* it
  lives is not a preference — `FilmRgbImage::from_linear` is `pub(in crate::algo)` and
  `AcesCgImage::new` is private to `working_space`, so a `pipeline/decode.rs` could mint
  neither without widening a boundary the project closed deliberately, and
  `nf-verification/film-rgb-export` (which depends on this task) is specified as
  consuming a `FilmRgbImage` where `map_nc_film_rgb_v1` would. So the decode mints the
  one existing typed boundary and the `→ AcesCgImage` step stays `working_space`'s.

  The cost is that the acceptance test stops being a tautology and becomes real: fresh
  arithmetic must come out **bit-identical** to `algo::reconstruct` driven by the
  equivalent explicit flags. Fusing the two passes (no `DensityImage` intermediate) is
  allowed because every step already rounds to f32 at the same points and Rust does not
  contract to FMA — so `mul_add` is banned here, along with dropping `+ offset` at zero
  and factoring the anchor out of the exponent.

  **(2) Placement A ships, provisionally.** `mid-at-base-offset(d = 0.62)`, contrast
  2.0, `scale [1, 0.84, 0.73]`, `offset [0, 0, 0]`; anchor `A = 0.99236375`. No
  reference density is resolved or read. `d`'s provenance is `anchor-rule`'s, the
  contrast split is `gamma-split`'s, and both values are expected to move.

  **(3) The new flow reads `DecodeParams`, not `ResolvedConfig.reconstruction` — which
  is why the recipe is refused wholesale for now.** Not reading the legacy object
  creates the accepted-and-ignored hole by construction, so under `--new-flow` a
  `--params` recipe carrying a `reconstruction` section, `--preset`, and `--dump-params`
  are all refused, naming `nf-core/recipe-schema` as where the new spelling arrives
  (`--dump-params` writes the resolved config *before* the render seam, so it would
  describe a chain the run did not select). Blunt and temporary, but it closes the exact
  hole `flow.rs` records as the audit's open question — the knee stated by a recipe or
  expanded from `--preset sigmoid-knees` — which only became decidable once the decode
  owned its own params. The decode's *surviving* knobs stay reachable by flag:
  `--density-scale`, `--density-offset`, `--density-gamma`, `--anchor-mid-offset`.

  Consequence for later: `roll` takes no conversion flags, so `roll --new-flow` can
  state no decode knob at all until the recipe spelling exists. Nothing regresses (it
  cannot render either), but `nf-core/subcommands` inherits the spelling, not just the
  plumbing — and `nf-calibration/scale-gamma-loop` should not assume it can drive a roll.

  **(4) Five quantities, named once, in the module rustdoc where the conflation would
  happen.** They are routinely collapsed into "dmax" or "white" and they are not
  interchangeable:

  | name | what it is | source | status |
  |---|---|---|---|
  | film base / `Dmin` | per-roll transmission of unexposed film | measured from the rebate | shipped; `D′ = 0` by construction |
  | leader `Dmax` | film **saturation** density | `--d-max`, `estimate --d-max-region` | today's `curve.dmax`; **not read** by this decode |
  | anchor `A` | the corrected density that renders to `1.0` | derived, `d + 0.7447/contrast` | **0.99236375**, reported |
  | diffuse white | scene white on a correctly exposed negative | datasheet, `d + 0.36` | **0.98** — a reference number, not an input |
  | content white `W` | a roll's bright end (red p97 of picture density) | `docs/spike/white-placement.md` | **not measured, not shipped** |
  | specular headroom | ~1 stop above diffuse white | where an HDR rendition lives | a consequence, not a knob |

  **What the white-placement shortlist needs from the decode, and what it does not.**
  Option **C** — the one the spike found pins both ends — needs *nothing* here: it
  solves `gamma = MID_GREY_OUTPUT_DECADES / (W − d)` from a **content** white and the
  spike puts that per-roll contrast in the look stage, so the decode stays fixed.
  Options **B** and **D** are the ones that would re-open an input to the decode's
  anchor. So `DecodeParams`'s anchor is a named type (`AnchorRule::MidAboveBase(d)`),
  not a bare `f32` — B and D add a variant instead of silently changing what a field
  means. That is naming, not mechanism: no white-reference field, no solved contrast, no
  measurement.

  **The leader `Dmax` has a third role nobody has retired.**
  `docs/spike/white-placement.md` wants a **guard** — white staying a measured margin
  below the leader's saturation, and below the ~1 stop of specular headroom. That is
  neither an anchor nor a content white, and `nf-retire/dmax-machinery` as written
  retires `estimate --d-max-region` wholesale. Recorded rather than acted on here: it is
  a reason the *measurement* may outlive the *anchor*.

- 2026-09-22: **landed — `src/algo/fixed.rs`, the refusal inventory, and the guide.**
  All five CI gates green (including the `nctool` suite); `cargo doc` unresolved links
  back at the 16-link baseline after two `#[cfg(test)]` intra-doc links were spelled
  as prose instead.

  **The equality holds, bit-exactly, and it was worth writing fresh to find out.** The
  fused single pass reproduces `algo::reconstruct` under the equivalent explicit
  configuration bit-for-bit over a vector covering the film base itself (`D = 0`, the
  `−0.0` case), a dead pixel, a negative sample, a denormal and the non-finite trio.
  Because it compares two implementations **on one host** it is cross-target safe with
  no `reachable_window` machinery — the usual libm caution applies to checked-in
  constants, not to this shape. Falsifiability both ways: a one-ULP move in `d` reds
  it, and a reference-derived placement on the same curve is distinguishable.

  It also allocates **one buffer where the legacy path allocates one and transforms it
  again** (no `DensityImage` intermediate). `nf-core/minimal-end-to-end` should read
  that off the code rather than inherit `RunProfile::Convert`'s arithmetic — nothing
  tests the memory model against the code.

  **The `Dmax` refusal covers all four spellings, `--no-d-max` included, and that is
  the tiebreaker applied rather than waived.** An identity value is one that asks for
  nothing *of a knob the branch has*; the fixed decode has no reference density at
  all, so "resolve it from nowhere" is a statement about a quantity that does not
  exist here. The knobs the decode genuinely reads stay reachable and have a
  falsifiable control test that they reach the seam.

  **The audit's worked instance is closed, and by a mechanism worth reusing.** The
  recipe/`--preset` hole could not be closed with a value rule (the default sigmoid
  *has* knees, so refusing a non-zero resolved knee would refuse every `--new-flow`
  run). Giving the stage its own params closes it from the other end: the new flow
  reads no resolved `reconstruction`, so the section is refused whole and `--preset`
  by presence. `docs/tasks/nf-core/knob-availability-audit.md` now records that.

  **Three things the next task inherits.**
  - Nothing **maps** the surviving flags onto `DecodeParams` yet. It cannot be
    accepted-and-ignored today (the seam refuses first), but
    `nf-core/minimal-end-to-end` owns the mapping and must not leave it.
  - `--density-gamma` beside the resolved *sigmoid* default is refused by `merge`,
    whose remedy is `--density-curve exponential` — which this flow accepts, so the
    advice is followable rather than circular. It does mean the decode's own contrast
    currently needs the curve named alongside it.
  - `--dump-params` is refused under `--new-flow`, because it writes the resolved
    legacy config *before* the render seam and would hand back a recipe that replays
    as a different picture. That removed the premise of `without_new_flow_nothing_moves`'s
    parity assertion; what survives is that the no-flag path is untouched and nothing
    named `new_flow` reaches the recipe.

  **A small trap, found by a test failure.** `--film-stock` is validated for *value*
  in `merge`, so under `--new-flow` an unknown stock now reports the availability
  refusal rather than "unknown film stock" — the availability row runs first, by
  design. Correct, but it means a test asserting merge's wording must use a valid
  stock name or it silently exercises the wrong rule.

- 2026-09-22: **two corrections and one number, after `nf-look/path-to-white`'s session
  flagged the anchor's edges.** Appended rather than edited into the entries above, per
  the append-only rule.

  **What this decode ships, stated as a number.** Under `mid-at-base-offset(0.62)` at
  contrast 2.0 — exactly what landed — all three rolls measured in
  `docs/spike/white-placement.md` land **0.55–1.28 stops below white** (Gold200 −1.28,
  Portra400 −0.88, Ektar100 −0.55). That is the intended shipped state, not a defect:
  `nf-reconstruction/anchor-rule` owns the rule and may move it. Recorded here because
  it is a property of *this* task's output, and because a per-channel highlight
  operator has nothing to act on under it (`nf-look/path-to-white`).

  **Correction to the entry above: all four white placements are reachable today, not
  just C.** The claim that "B and D would re-open an input to the decode's anchor" is
  too strong. `mid-at-base-offset` resolves `A = d + M/gamma` (`M` =
  `MID_GREY_OUTPUT_DECADES`), so:

  - **C** solves `gamma = M / (W − d)`, which substitutes in to give `A = W` **exactly**
    — the mid-anchored and content-anchored spellings are one placement.
  - **B** (`A = W` at the shipped contrast) is the same identity read the other way:
    `d = W − M/2`. On Gold200's `W = 0.800` that is 0.4276 — and the spike's own table
    already prints `d = 0.428 / 0.538 / 0.488` for the three rolls, so it had computed
    this without naming it.

  Both verified against the binary: `--density-curve exponential --anchor-mid-offset
  0.4276 --density-gamma 2.0` and `--anchor-mid-offset 0.62 --density-gamma 4.1374`
  both render at exit 0 on the legacy flow. So what B/C/D actually need is **a
  measurement of `W` and somewhere to put it**, not a decode change. The decode's
  anchor only has to grow a variant if it is to *consume* that measurement itself
  rather than take a hand-set number — which is a smaller claim than the one above,
  and leaves `AnchorRule` correctly sized either way.

  **A wrinkle for that tuning method.** `--density-gamma` is on the surviving list and
  stays reachable, but beside the *resolved sigmoid default* `merge` refuses it with
  "its mid-density slope is `--sigmoid-contrast` (or pass `--density-curve
  exponential`)". Under `--new-flow` the first remedy is itself refused and the second
  works, so the advice is followable but half dead. It dissolves when the new flow's
  default curve moves (`nf-core/minimal-end-to-end` / `default-flip`); until then the
  tuning line needs `--density-curve exponential` named alongside.

  **No `DmaxSource` was flipped, so the `Auto`-keyed predicates have not moved.**
  `src/types.rs` is untouched by this task: the new flow still *resolves* the legacy
  config exactly as before and simply does not read it. So `measures_over_region` and
  `region_reaches_a_rendered_pixel` — and the "IR preserved but not used" warning the
  second one suppresses, with `--strict` behind it — behave identically on both flows
  today. Whoever moves the default owns that change. One pre-existing detail they
  should know: that warning is emitted *before* the render seam, so a `--new-flow` run
  can already print it and then exit 4.
- 2026-09-22: **review round applied** — fixes only, no commits. What the two engines
  found that was real, and what changed.

  **Two of the four bit-identity rules were vacuous, and the module docs claimed one
  test held all four.** Measured by breaking each rule and bit-comparing over the
  12-sample vector: dropping `+ offset` moves **0 of 12** at the shipped `[0, 0, 0]` and
  9 of 12 at a non-zero offset; an FMA moves **0 of 12 at either**; the density's f32
  rounding moves 1 of 12 at the shipped offset and 0 at a non-zero one; the factored
  anchor moves 6 and 9. So the equality test now runs at **two** offsets (`DENSITY_OFFSET`
  plus a non-zero `PROBE_OFFSET`, because neither alone holds both of the first two
  rules), the FMA has a test of its own sweeping the band just under the film base where
  the plain and fused forms part, and the docs state the measured split instead of the
  claim. Each of the four mutants was confirmed to red the suite.

  Three things worth keeping from that. The FMA test **sweeps** rather than pinning a
  sample: the witness is reached through `log10f`, which may differ by a ULP across
  targets, so a hard-coded one could witness here and not in CI — it asserts every swept
  sample equals the plain form and that the band distinguishes the two at all, so it
  cannot go vacuous silently. The rule-1 witness is a **single** sample (`0.5 / 0.9`),
  now noted on `scan()` so an edit to that vector cannot retire it unnoticed. And one
  mutation stays invisible on purpose: skipping `+ offset` *only* when the constant is
  zero is unobservable in the **output** — the `−0.0` it normalizes cannot survive the
  anchor subtraction — so that rule is carried by the non-zero pass and goes live when
  `nf-calibration/offset-question` moves the constant. The older `−0.0 → +0.0` wording
  described the staged path's stored density, not this decode's pixels.

  **`scale` is now guarded positive, not merely finite** (`check_params`), matching
  `cli::validate`'s `positive("--density-scale", …)`. Zero rendered that channel flat —
  finite, in range, tripping no counter — and negative reversed its density ordering,
  against the strictly-increasing contract. `offset` stays finite-only: it is signed.

  **Four refusal messages under `--new-flow` were wrong and are fixed.**
  `--sigmoid-contrast`'s remedy said `--density-gamma`, which `merge` refuses beside the
  resolved sigmoid default, whose own first remedy is `--sigmoid-contrast` — a two-step
  loop; it now names `--density-curve exponential --density-gamma`, verified to work in
  one step. `DMAX_REASON` said only what is gone, reading as "you need leader data"; it
  now states that mid-grey is pinned above the **film base**, which every scan carries.
  `ANCHOR_RULE_REASON` diagnosed all three anchor rows as "chosen because it is
  reference-free", which `--anchor-black-floor` **also** is (`reads_reference()` is false
  for `BlackAtBase`), so it is split in two. And "nothing reaches the new flow through a
  knob it does not read" is scoped to the reconstruction: `print.*`, `output.*` and
  `measure.*` are still accepted and read by nothing.

  **Stale-by-this-change prose corrected**: the seam rustdoc and `pipeline::chain`'s
  module doc both said the decode feeding the chain was missing (only the destination
  is), `chain`'s producer-agnostic test comment said the new flow will take those density
  curves (it will not — it decodes through `DecodeParams` and refuses a recipe
  `reconstruction`), `docs/TASKS.md` twice called this task "defaults and wiring, not new
  arithmetic" (the task file had already been corrected), and `docs/design-spec.md` §10's
  module tree had no `algo/fixed.rs`.

  **`docs/using-nc.md` carried a claim the binary contradicts.** It listed
  `--density-gamma` among the knobs that "still work" under `--new-flow`; bare, it exits
  **2** (`merge` refuses it beside the resolved sigmoid default) where the other three
  reach the seam at exit 4. The guide is now qualified, and
  `the_fixed_decodes_own_knobs_stay_reachable_under_the_new_flow` gained bare-flag cases
  — its base argv injects `--density-curve exponential`, so it could not observe this.

  **Four remedy defects reachable under `--new-flow` are `nf-core/knob-availability-audit`'s,
  not this task's**, agreed with that session: `--sigmoid-contrast`'s own `instead:`
  (fixed here), `merge`'s `--density-gamma` arm, the two knee arms, and the `cli.rs`
  anchor-placement guard's remedy. The **knee** case is the worst, and this task's design
  creates it: the rows deliberately accept a zero knee as an identity value (to keep the
  flags-win reset usable), and `merge` then refuses `--sigmoid-toe 0` beside a resolved
  exponential with "pass `--density-curve sigmoid`" — a curve the rows refuse. Its
  parenthetical is an aside about slope *naming*, not a second remedy: adding
  `--density-gamma 2` leaves the message byte-identical. So it is the only one of the
  four offering no working action, and the action that does work — drop the knee flag —
  is never stated. Legacy is unaffected (`--density-curve sigmoid --sigmoid-toe 0`
  renders at exit 0). The general lesson from that session: bounding the search to "only
  rules keyed on an accepted knob's value can fire" is correct, but walking it by
  enumerating **knobs** misses the reachable **values** — a refused knob's accepted
  identity value is in the reachable set.

  **Not applied here: the two `CLAUDE.md` items** (the module map's missing
  `algo/fixed.rs`, and the availability-refusal sentence, which now needs the third
  recipe-section provenance and no longer holds for `roll`). This agent may not edit
  `CLAUDE.md` on an agent's say-so, so both went back to the user with the exact wording.

- 2026-09-22: **done.** Landed as `src/algo/fixed.rs` — `DecodeParams` / `AnchorRule` /
  `DecodeReport` plus `decode()`, written fresh and fused into one pass, living in
  `algo` because `FilmRgbImage` is mintable only there. Verified **bit-identical** to
  `algo::reconstruct` under the equivalent explicit configuration, and that equality
  re-run across five `DmaxSource` values is what proves no reference density is read.
  Beside it: the reconstruction half of the new-flow knob inventory in `flow.rs`, and a
  raw-JSON witness refusing a recipe `reconstruction` section. All five gates green.

  **What a dependent task needs.**
  - **`nf-core/minimal-end-to-end` owns three loose ends.** Nothing yet *maps* the
    surviving flags (`--density-scale`, `--density-offset`, `--density-gamma`,
    `--anchor-mid-offset`) onto `DecodeParams` — harmless only while the seam refuses
    first. `--density-gamma` still needs `--density-curve exponential` named alongside
    until the new flow's default curve moves. And the fused decode allocates **one
    buffer fewer** than the legacy staged path, so its `RunProfile` must be measured
    rather than inherited from `Convert`.
  - **The recipe refusal is blunt and temporary.** `--new-flow` refuses a `reconstruction`
    section whole, so `roll --new-flow` can state no decode knob at all until
    `nf-core/recipe-schema` gives the new stages a spelling. A per-frame overlay is *not*
    witnessed — unreachable today, live the moment the roll seam opens
    (`nf-core/subcommands`).
  - **Four remedy defects reachable under `--new-flow` are `nf-core/knob-availability-audit`'s**,
    not this task's: `merge`'s `--density-gamma` arm, its two knee arms, and the `cli.rs`
    anchor-placement guard. The knee pair additionally *closed a cycle* with this task's
    own `--density-curve` row; that half is fixed here (the knee rows now fire beside a
    typed knee-less curve, where "Drop it for now" is the working remedy) and pinned by
    `a_knee_flag_beside_a_knee_less_curve_is_refused_before_merge_can_loop`.
  - **The end-to-end acceptance criterion moved and is unscheduled.** This task's original
    "under `--new-flow` with no other flags the pixels are identical to the old flow" is
    only testable once the seam opens; it was replaced by the function-level equality.
    Whoever opens the seam should re-state it there.

## anchor-spike

**Status:** done
**Updated:** 2026-09-21

- 2026-09-20: filed after the three-way converter measurements
  (`docs/reports/three-way-gold200.md`). Goal: does a diffuse-white anchor earn its
  place? Runs against today's binary — `--anchor-white-at-reference --d-max D` is the
  candidate, `--anchor-mid-offset` the control — so it can answer `anchor-rule`'s
  highlight-vs-mid question before the rule has to be chosen.
- 2026-09-21: **decode-side half run on three rolls — under the proposed anchor no roll
  reaches white, so a highlight operator would have nothing to act on.** Scripts and raw
  data in `../temp/anchor-spike/`. Bases measured per roll with `estimate --grid` (cell
  spreads 0.007–0.021, benign gradients); densities are `D' = scale·D` with the shipped
  `[1, 0.84, 0.73]`, read on **red**, where `scale = 1` and `D' = D` — the units the
  anchor is stated in.

  The anchor arithmetic is confirmed against the binary: `mid-at-base-offset(0.62)` at
  gamma 2 reports `anchor_value: 0.99236375`, matching `d + MID_GREY_OUTPUT_DECADES/2`
  to seven figures, and sitting 0.012 above the datasheets' diffuse white at 0.98.

  | roll | n | red p97 median | p97 p90 | vs anchor | renders at | stops short |
  |---|---|---|---|---|---|---|
  | 2026-09-18-Gold200 | 35 | 0.671 | 0.800 | −0.193 | 0.412 | **−1.28** |
  | 2026-09-14-Ektar100 | 32 | 0.710 | 0.910 | −0.083 | 0.683 | −0.55 |
  | 2026-09-11-Portra400 | 12 | 0.546 | 0.860 | −0.133 | 0.543 | −0.88 |

  **(1) The open-loop diagnosis is quantified.** Every roll's bright end lands 0.55–1.28
  stops below white. Nothing enters the region a per-channel operator compresses, so
  `nf-look/path-to-white` would be inert under this decode regardless of its form — the
  two spikes are coupled through this number, not only through the argument.

  **(2) One fixed `d` cannot serve the three.** To put each roll's bright end at white
  they would need `d` = 0.428 / 0.538 / 0.488 — a spread of 0.110 density, **0.73 stops**.
  So the reading is not simply "0.62 is too high": any single value leaves the rolls that
  far apart at the top, which is the trade a content-referenced anchor removes and the
  faithfulness it gives up.

  **(3) Per-frame would move far more than per-roll.** Within-roll spread of per-frame
  red p97 is 0.40 / 0.40 / 0.96 in `D'` — Portra400's widest frame sits 1.3 while its
  narrowest sits 0.33. Confirms roll granularity as the unit; a per-frame anchor would
  re-expose every frame.

  **Aside, and not this task's:** `--display-tone none` refused a Gold200 frame outright
  because the **film holder** renders above reference white (opaque → high density →
  bright positive). `effective_area` reported `holder: 0` on all four edges with
  `holder_applied: false` and only its 5% static inset (167 px of 3343), which does not
  reach the holder on this scan. Relevant to `film-base/holder-cap-contamination`, and a
  trap for any measurement taken on a full frame rather than an inset one.
- 2026-09-21: **done**, having costed a shortlist rather than picked from it. The
  numbers live in [`docs/spike/white-placement.md`](../spike/white-placement.md);
  the summary below is the decision trail. Four
  placements, with what each gets right on 2026-09-18-Gold200 (`W` = 0.800, `d` = 0.62):
  **A** fixed anchor pins mid and reaches 0.412; **B** content white reaches white and
  puts a datasheet mid at 0.437; **C** pins both by solving
  `gamma = MID_GREY_OUTPUT_DECADES / (W − d)` = 4.15; **D** is C with a noise ceiling,
  sliding toward B when the cap binds. C is the only one that pins both ends, and
  structurally — two free parameters against two constraints. It stays inside the design
  because `gamma` already splits, so its per-roll contrast (2.31× / 1.43× / 1.73× over
  the 1.8 linearization) is the look stage's knob and the decode stays fixed. Its cost is
  that it asks Gold200 for more contrast than any converter used on that roll.

  The rendered ranking is deliberately **not** here — under A nothing reaches the region
  a per-channel operator acts in, so a render now would compare four configurations of
  which one is inert. It moves to
  [`nf-calibration/anchor-comparison`](../tasks/nf-calibration/anchor-comparison.md),
  which waits on the operator.

## anchor-rule

**Status:** not started
**Updated:** 2026-09-22

- 2026-09-19: created with the new-flow plan. Goal: one anchor rule, with a value for `d`.
- 2026-09-22: **how a content-referenced rule would be spelled, decided before the task
  starts.** Not which rule — that is still this task's call — but where its parts live if
  it reads content. The **rule** stays a look knob and the **measurement** it consumes
  becomes a third `calibration` key beside `film_base` and `dmax`; that is the
  `AnchorPlacement` pattern already in the tree, which design-spec §8 states as "the
  anchor is the rule for what the reference places" with only the measured value leaving.
  `core/calibration-recipe-section` has been asked to keep that section open rather than
  model it as a closed pair.

  Four things that follow, now in the task file: record the **percentile with its value**
  (p95/p97/p99 differ by about a stop on one roll, so a bare scalar loses the definition)
  plus probably the per-frame spread, since telling a flat roll from a wrong `d` is
  exactly what candidate D exists for and candidate C cannot do; **nothing measures it
  today** — `estimate` reads one frame, a roll percentile means reading the roll with
  provenance and confidence, i.e. `core/base-acquisition-planner`'s cascade; it is
  **content-derived, not reference-derived**, unlike the rebate and the leader; and the
  placement **stays an enum** whatever is picked, because `nf-calibration/anchor-comparison`
  has to render the candidates to rank them.

  **What the candidates actually cost, sharpened by the `fixed-decode` session the same
  day.** The placement is a two-parameter family `(d, gamma)` and all four candidates are
  points in it, because `A = d + MID_GREY_OUTPUT_DECADES / gamma` can be solved either
  way: the hybrid fixes `d` and solves `gamma = M / (W - d)` to get `A = W`; the level
  move fixes `gamma` and solves `d = W - M/gamma`, which at gamma 2.0 on Gold200's
  `W = 0.800` is **0.4276** — and `white-placement.md`'s own table already prints
  `d = 0.428 / 0.538 / 0.488` for the three rolls. So **every candidate is reachable on
  today's binary** with `--density-curve exponential --anchor-mid-offset <d>
  --density-gamma <g>` (the curve selector is required: `merge` refuses `--density-gamma`
  beside the resolved sigmoid default). B and C were rendered at exit 0 to confirm.

  **The consequence is the point of this entry.** B/C/D do not need a decode change —
  they need a *measurement* of `W` and somewhere to put it. The anchor grows a variant
  only if it is to **consume** that measurement rather than take a hand-set number, which
  is the same distinction `film_base::estimate` draws by taking a *resolved*
  `&FilmBaseSource` rather than the params object.

## gamma-split

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: split `gamma` into calibration and look.

## curve-endpoint-warning

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: warn when the curve's endpoint is unreachable.

## mono-decode

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: where black-and-white fits the new chain.
