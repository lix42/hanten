# Hanten — nf-core Progress Log

Execution log for the `nf-core` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

The new chain is the only one. The epic built it behind `--new-flow` — the selector,
the stage module tree, a minimal end-to-end render, the knob audit, the recipe — and
**`default-flip`** (2026-09-27, `pipeline_version` 8) made it the default and deleted
the old chain, its presets, its print stage, its recipe, its sidecar and
`ultrahdr-sys`. `--new-flow` is now a removed-flag error. **`report-contract`**
settled the report (`chain`, the `recipe` echo and its hash, no sidecar) and telemetry
schema 9. **`subcommands`** settled `roll`'s per-frame overrides: a frame resolves as
`convert` would, and an override that changes a roll-wide value (`cli::ROLL_WIDE`)
warns naming both values. Open: `buffer-strategy`, `three-step-pipeline`.

**Adding or changing a knob** (what used to be the availability tables in `src/flow.rs`,
deleted by the flip):

- **A knob is a `*Overrides` field (`cli.rs`), a field on its recipe section**
  (`src/recipe.rs`, or the stage's own params where the section *is* that struct), a
  `recipe::merge` arm, and usually a `recipe::validate` rule.
  `every_flag_reaches_the_recipe` reads the flag surface back out of clap, so a flag
  that reaches no recipe field reds the gate; `NON_KNOB_FLAGS` lists the operational
  and input-only ones.
- **A retired flag stays parsable, hidden, and is refused** by
  `cli::reject_removed_flags` with a message naming what replaced it, before `merge`;
  `every_hidden_convert_flag_is_refused` pins that a hidden flag cannot parse and do
  nothing.
- **A retired recipe key** is refused by `recipe::check_body` by name, with where its
  knob went. A recipe without `"recipe_version": 2` — every sidecar and `--dump-params`
  file written before `pipeline_version` 8 — is refused whole; there is no converter.
- **The film master refuses a stage by stage, never by knob**
  (`recipe::destination`): each stage's default and identity are spared, so a flag can
  clear what a recipe asked for.

**What a dependent epic needs to know.** Every stage is
`apply(input, &Params) -> Result<Output>`, pure. The stage **order is carried by the
types**, not by the composition function: each stage's input is the previous one's
output, and each boundary type can be minted only inside the module that produces it,
so an out-of-order chain does not compile. Crossing a boundary **moves** the buffers,
so a type per stage costs no allocation. Each stage's `Params` is a struct — not an
`Option`, because "this stage is off" is deliberately not expressible. Fit gamut owns
the change of primaries into the destination's gamut (`DestinationGamut`, carried by
`FitGamutParams`, which has no `Default`), and its recipe section is the separate,
empty `recipe::FitGamut`. The IR plane rides the whole chain and leaves it with the
image through `DisplayReferredImage::into_parts`. `GradedImage` is the boundary the
SDR/HDR split splits *from*.

- **The destination decides the output path** (`cli::OutputTarget`): `-o out`
  completes to the container's suffix, and a stated suffix the destination does not
  write is refused, offering the axes that would write it.
- **No sidecar; the report is the record** (`report-contract`): a `convert` report
  echoes the `recipe` and `identity.params_hash` hashes its `--dump-params` bytes (so
  does telemetry's); each roll frame hashes the recipe it ran. The `chain` block holds
  the decode facts, each stage's `applied`, the stages' values and the destination. A
  sidecar a pre-flip run left beside a replaced image is removed, and reported.
- **Stages are named once, in `crate::stage::StageKind`**, and timed through a
  `StageClock` the orchestrator passes the chain; telemetry schema 9's `timing_ms` has
  one field per stage. A new stage is a `StageKind` member, a `TimingInfo` field, and a
  `clock.time` call where it runs.
- **Memory profiles are per buffer shape** (`RunProfile::{U16Tiff, F32Tiff, Avif,
  GainMapJpeg}`); a buffer added to a stage must move its arm.

## new-flow-flag

**Status:** done
**Updated:** 2026-09-20

- 2026-09-19: created with the new-flow plan. Goal: the `--new-flow` selector.
- 2026-09-20: shipped. `src/flow.rs` carries the whole migration surface — the
  `Flow` enum, the availability tables, and the render seam — so
  `nf-core/default-flip` deletes one file plus the arg fields rather than hunting
  `if` arms through `cli`.

  **The three open questions, decided.**
  - *Spelling:* `--new-flow`, a presence flag (user's call). A named
    `--pipeline <x>` selector would outlive the thing it selects; the flag has a
    written expiry.
  - *Where the branch is taken:* in `convert_frame`, immediately before the
    output-preset render dispatch. Decode and film base are **shared by both
    flows** — the new design keeps them — so that dispatch is the honest seam,
    and it is the arm `nf-core/stage-skeleton` fills rather than moves.
  - *Does `roll` accept it:* yes (user's call). `RollArgs` gets the flag and every
    frame resolves the selected chain.

  **What `--new-flow` does today.** Nothing renders: the chain it selects has no
  stages until `nf-core/stage-skeleton`, so the seam returns
  `NcError::Unsupported` (exit 4) naming that task. Exit 4 rather than 2 because
  the command line is well-formed and the config resolved — the same distinction
  `Resource` draws for the memory gate. **`roll` refuses once**, after the plan
  resolves and before the first decode, rather than per frame: the memory gate is
  per frame because its verdict depends on each frame's own size, while this
  condition is frame-independent and already known, so per-frame handling would
  decode all 25 frames of a real roll to print one identical error 25 times. (A
  first version did exactly that, at exit 1; caught in review.) Per-frame *config*
  refusals still come first, at exit 2.

  **The refusal mechanism, and why it is evaluated in two places.** One message
  builder over two `const` tables: presence-keyed on `&ConvertArgs`, value-keyed
  on `&ResolvedConfig`. The presence half runs **before `merge`** — CLAUDE.md's
  ordering-across-gates trap: a presence rule after `merge` is unreachable on
  every command line `merge` refuses first, and the user then gets a remedy
  naming a knob this flow rejects. The value half runs **first inside
  `validate_convert`**, ahead of every rule that reasons about the shipped chain,
  for the same reason. An integration test drives both through the binary with no
  film base and asserts the availability message wins over `validate`'s
  least-specific "no film base selected" — a test calling a rule directly
  exercises the rule and never the ordering.

  **`roll` reaches only the value half**, since it accepts no conversion flags: a
  roll knob is always a resolved value, from the shared recipe or a per-frame
  overlay. Those are **two** validate sites, composed once into
  `cli::validate_with_flow` rather than called twice — the `OutputPreset::is_atomic`
  lesson about a rule spread over three call sites. Both sites are pinned.

  **Two provisional seed entries only**, so the mechanism is exercised through the
  binary rather than unit-tested in isolation: `reconstruction = simple` (value,
  `Never`) and the sigmoid knees (flag, `NotYet`). Both verdicts are ones
  `docs/design-update.md` already settled. The inventory is
  `nf-core/knob-availability-audit`'s product; adding a row there changes no
  message, ordering or call site.

  **The knee rule reads the value typed, not mere presence** — `--sigmoid-toe 0`
  asks for a knee-*less* curve, which is bit-exactly the straight line the new flow
  decodes with, so refusing it would break the flags-win reset the tiebreaker
  protects. A first version refused it by presence; caught in review, and pinned
  now by a test that the zero knee reaches the seam.

  **A value rule cannot outrank `merge`** — it has nothing to read until `merge`
  resolves a value, so `--new-flow --reconstruction simple --preset sigmoid-flat`
  got merge's legacy "drop `--reconstruction simple`, or drop the preset" before the
  flow's own refusal. Caught by the PR's Codex reviewer. Fixed with the
  `OutputPreset::is_atomic` dual-rule shape: `simple` sits in **both** tables, the
  flag row pre-empting `merge`, the value row still covering recipe provenance. A
  unit test pins that the pair's verdicts agree, and an integration test pins the
  ordering against both merge arms with a no-flag control. What stays open: a knob
  arriving **only** through a recipe cannot be pre-empted, since the recipe's value
  is not the resolved one until the flags have won.

  **Known hole, handed to the audit with its reasoning.** The knee rule sees only
  the flag, so the same knee from a recipe or from `--preset sigmoid-knees` is not
  refused. A value rule cannot just be added beside it: the default sigmoid *has*
  knees (`toe: 0.2`), so "refuse a non-zero resolved knee" would refuse every
  `--new-flow` run before it reached the seam — which is the audit's own "a knob
  the user never typed" question, and it cannot land before
  `nf-reconstruction/fixed-decode` moves the default. Recorded there as a worked
  instance, and in `FLAG_ENTRIES`' rustdoc. Unreachable today (the seam refuses
  every `--new-flow` render anyway); it becomes real the moment `stage-skeleton`
  lands an identity chain, so the help text and the guide claim only that knobs
  the chain cannot honour are refused **as the inventory is assembled**, not that
  the set is complete.

  **Not in the operational class, and the docs say so.** `--new-flow` is CLI-only
  like `--report` / `--telemetry` / `--max-memory`, but for a different reason —
  it selects which knobs exist — and unlike them it *will* change pixels. So it
  breaks "same inputs + params ⇒ identical output" outright, which is the argument
  for keeping it out of the recipe rather than merely out of the image. Recorded
  in CLAUDE.md, `docs/using-nc.md` §11 (a subsection of its own, verified by
  running the binary) and one clause in design-spec §9 — the spec deliberately
  does **not** specify the flag, since it is scaffolding, but its claim that the
  operational exception covers "flags that touch no parameter at all" would
  otherwise have read as exhaustive.

  **Deferred, with a home.** A `--new-flow` render will not be reproducible from
  its own sidecar, since the flow is not in `params`. The right home is the
  sidecar's `meta` block (provenance, and `SidecarEnvelopeIn` keeps `meta` as an
  ignored raw `Value`, so older builds tolerate a new field) — moot while nothing
  renders, so it belongs to `nf-core/report-contract` /
  `nf-core/minimal-end-to-end`.

  **Toolchain note:** the local stable was 1.94 against CI's 1.98, and 1.94
  reports a `nonminimal_bool` warning on pre-existing code in `cli.rs` that 1.98
  does not. Updating first (CLAUDE.md's rule) is what kept the gate readable;
  judging the diff on 1.94 would have shown a red gate that has nothing to do
  with it.

## stage-skeleton

**Status:** done
**Updated:** 2026-09-21

- 2026-09-19: created with the new-flow plan. Goal: the new stage module tree.
- 2026-09-21: shipped. `pipeline::chain` composes `scene_correction` -> `look` ->
  `fit_range` -> `fit_gamut`, every stage an identity pass, over the shared
  `working_image::WorkingBuffer`.

  **The four decisions, and why.**
  - *Scope: the chain only.* No recipe sections and no report fields, because
    `nf-core/recipe-schema` and `nf-core/report-contract` both **depend on this
    task** and own those surfaces. The task file's "appears in the chain, in the
    report, and in the recipe" describes the migration's end state, not this
    change; reading it literally would have pre-empted two tasks waiting on it.
  - *Flat modules, named for the job* (`pipeline/scene_correction.rs`, not
    `pipeline/nf/`). `nf` is scaffolding vocabulary with a written expiry, like
    `--new-flow` itself, so a directory named for it buys separation now and costs
    a rename later — and CLAUDE.md records that renames here have long tails
    (backticked prose paths and intra-doc links rot with every gate green).
    Retirement deletes the *old* path, so the new one should already carry its
    permanent name.
  - *The seam stays shut.* The chain exists but `--new-flow` still exits 4. The
    alternative — compose the identity chain and refuse at the encode — is a full
    render thrown away on a real 5000 dpi scan to reach the same message.
  - *A distinct boundary type per stage, each consume-and-return.* The strong form
    from the task file, and the `AcesCgImage` pattern: private field, minting
    private to the producing module. **The usual objection does not apply here** —
    a type per boundary costs no allocation when each stage *moves* the buffers out
    of its input, which is also a partial answer to `nf-core/buffer-strategy`'s open
    "are identity stages free?": yes, if they move.

  **The shared buffer is a DRY call with a reason, not a reflex.** Four boundary
  types wrapping one `WorkingBuffer` rather than four hand-rolled copies of the
  `render_split::AdjustedAcesCgImage` block. Per the `rust-best-practices` skill's
  new §1.8: what a full-frame working image *is* (interleaved RGB, the carried IR
  plane, the length invariants, "a Debug never prints pixels") is **one piece of
  knowledge** used four times. The four wrappers stay separate because they encode
  *different* positions in the chain and will diverge as their stages gain
  parameters — deduplicating those would be the coincidental kind.

  **Params are empty structs, not `Option<T>`.** "This stage is off" is deliberately
  not expressible: `nf-look/stage` already settled that an empty stage reports as
  empty, not absent. It also means filling a stage never changes its signature.

  **Five `#[allow(dead_code)]`, and the placement is load-bearing.** One at
  `chain::render` and four on `DisplayReferredImage`'s accessors, each naming
  `nf-core/minimal-end-to-end`. Everything reachable *from* an allowed item is live,
  so `WorkingBuffer`'s own accessors need none — which is the argument for keeping
  each boundary's accessor surface minimal and adding to it only when a stage
  actually needs it. No crate-level or broad allow.

  **What the order test does and does not claim.** While every stage is an identity,
  reordering `chain::render` is **unobservable at runtime** — any order produces the
  same bytes — so a test asserting the order would be vacuous, which is the trap
  CLAUDE.md records for pinned-contract tests. The order is carried by the types
  (each stage takes the previous one's output, mintable only in its own module), and
  the test says so explicitly rather than pretending to check it. Real order
  coverage arrives with the first stage that does something.

  **The identity tests were falsified, not assumed.** Temporarily making `look`
  move one sample by a single ULP reds `the_chain_is_a_bit_exact_identity` and
  `the_chain_is_producer_agnostic`, and the structural tests correctly stay green.
  The comparison is on `f32::to_bits`, not `==`: `NaN != NaN`, so a value compare
  would pass over exactly the non-finite samples the contract says ride through
  untouched.

  **The seam's message changed, so four claims about it were stale.** The old text
  said the chain "has no stages yet (`nf-core/stage-skeleton` fills them)" — false
  the moment this landed. CLAUDE.md's negation grep found it in `flow.rs` (message
  and module rustdoc), a comment at the `cli.rs` seam, three assertions in
  `tests/pipeline.rs`, `docs/using-nc.md` twice and `docs/nf-migration.md`. All
  updated to name `nf-core/minimal-end-to-end`.

  **Re-verifying `using-nc.md` against the binary found a pre-existing error**
  (from `new-flow-flag`, not this change): §11 showed the *value*-row message for
  `--new-flow --reconstruction simple`, but that command trips the **flag** row and
  prints the shorter one. Fixed, and the recipe-provenance case that does produce
  the value-row wording is now shown beside it — verified by running both.

- 2026-09-21 (review round): eight findings from `/code-review`, all real, all fixed.

  **The exit boundary could not be moved out of.** `DisplayReferredImage` shipped
  with `&`-accessors only, while `io::encode` takes `&LinearImage` — so
  `nf-core/minimal-end-to-end`, the task those `#[allow(dead_code)]` comments name
  as the consumer, would have had to **copy** a full-frame buffer at the hand-off
  (~0.9 GB on a 74.6 MP scan, on top of the modelled peak). Every *interior*
  boundary moved, so `chain.rs`'s "crossing each boundary moves the pixel buffers,
  so a type per stage costs no allocation" was true everywhere except the one place
  it mattered most. Fixed with `DisplayReferredImage::into_linear` (mirroring
  `AcesCgImage::into_linear` at the other end) plus a test that the exit loses
  neither pixels, dimensions nor the IR plane. **The lesson for a typed chain: the
  boundary that leaves it needs the consuming unwrap most, and is the one easiest
  to forget, because nothing inside the chain exercises it.**

  **The negation grep failed because it was scoped.** The seam's message changed, and
  the CLAUDE.md-prescribed grep for the falsified claim was run with
  `grep -v '^docs/TASKS.md'` — excluding the authoritative plan file, which carried
  the claim verbatim. Four more sites used *different wording* for the same claim
  ("gives the new flow stages to run", "replaces the not-implemented seam",
  "needs once there is a chain to run", "has no render stages yet") and so matched
  none of the greppable phrases. **Grep the claim's meaning across every path,
  including the ones assumed to be status-only; a path exclusion is how this check
  fails silently**, and a second pass with no `-v` found the remainder.

  **A falsifiability test that tested nothing.** `a_stage_that_moved_a_pixel_would_be_caught`
  never called `render`: it cloned a bit vector, perturbed an element and asserted
  the clone differed — unconditionally true. It proved `Vec<u32>` comparison works.
  Renamed to `a_one_ulp_move_in_the_rendered_output_is_visible`, it now renders and
  perturbs the *output*, asserts the perturbed sample is finite (on a NaN `next_up`
  returns a NaN and the assertion would pass for the wrong reason), and its comment
  states plainly that pinning the *detector* is not the same as proving a stage
  non-identity — which remains the by-hand check recorded above. **A test named for
  a guarantee it does not provide is worse than no test**, and this one was written
  in the same change that warned about vacuous pinned-contract tests.

  **A log entry that described work not done.** The entry above said the guide now
  shows the recipe-provenance case "beside it — verified by running both"; only a
  prose sentence had been added, no console example. Since progress logs are
  append-only, the fix was to make the claim true — the example is now in
  `using-nc.md` §11, run and pasted verbatim — rather than to rewrite the history.

- 2026-09-21 (second review round): ten findings from the two-engine loop, one of
  them a corrected claim in the entry above.

  **The chain is now fallible, and the "filling a stage never changes its
  signature" claim above was wrong.** Both the entry above and the Epic summary said
  filling a stage would not need re-plumbing, while all four `apply`s and
  `chain::render` were infallible — and *every* counterpart in the shipped code is
  fallible: `render_split::display_source`, `sdr::render` (whose
  `render_pixel_checked` errors on a non-finite sample) and `hdr::render_linear` all
  return `Result`. The first stage to gain arithmetic — `nf-display-stages/fit-range`
  with reinhard plus that check — would have changed four signatures, `render`, every
  call site and every test here. Fixed by returning `Result` from each stage and from
  `render` now, while the identity stages pay only an `Ok` wrapper. **The lesson: a
  task whose whole product is a boundary must take the shape of the *filled* stage,
  not of the placeholder** — an empty `Params` struct was reasoned about exactly this
  way and got it right; the error type was not. `chain::render`'s "non-finite samples
  ride through untouched" is now scoped to the identity chain, since a filled fit
  range may legitimately refuse such a sample.

  **A rustdoc justified carrying the IR plane with a defect that is not this one.**
  `DisplayReferredImage::ir` claimed `--export-ir` and the strict-promotable "IR
  preserved but not used" warning "depend on the plane surviving to the end of a run",
  and cited CLAUDE.md's 22-of-25 suppression. Both false: `--export-ir` writes from
  the **decoded** image pre-render (`cli.rs`, and the memory model says so), both IR
  warnings are derived pre-render, and today's `pipeline::sdr` drops the plane
  outright (`LinearImage::new(w, h, rgb, None)`) while `--export-ir` keeps working on
  every display preset. The 22-of-25 defect was a caller re-deriving "was IR
  consumed?" instead of reading `BaseEstimate::ir_mask_applied` — a returned-fact
  problem in `film_base`, not a plane dropped at a type boundary. The plane is still
  carried, for the true reason: the design says carry it, and the chain must not be
  the thing that loses it. **A borrowed justification is worse than none** — it would
  have pointed `nf-core/minimal-end-to-end` at routing `--export-ir` off the chain's
  exit, either duplicating a plane already live on the decoded image (an unmodelled
  4 B/px full-frame buffer) or moving the export after the render.

  **The four `DisplayReferredImage` accessors are gone.** Their `#[allow(dead_code)]`
  comments named `nf-core/minimal-end-to-end` as the consumer, but the previous round
  had already given that consumer `into_linear()` — and a consumer cannot both borrow
  through `rgb()` and consume through `into_linear()`; after the hand-off it reads
  `linear.rgb`. So the allows named an impossible consumer, which is exactly the house
  rule's failure mode rather than its satisfaction. Deleting them left
  `WorkingBuffer`'s own four accessors unreachable, so those went too: the chain's
  tests read through the exit, and a stage that needs to borrow adds back what it
  needs. `the_ir_plane_and_the_dimensions_ride_through` then said exactly what
  `leaving_the_chain_preserves_everything_the_encoder_reads` says, so the two were
  merged into the latter (`an_ir_free_input_stays_ir_free` stays as its control).

  **The falsifiability control compared against the wrong baseline.** It perturbed the
  *output* bits but asserted `!=` the *input* bits, so it would pass on any chain that
  stopped being an identity — precisely when it stops exercising ULP detection. It now
  compares the perturbed output against the unperturbed output.

  **The minting restriction is pinned now.** The task asked for it and nothing
  asserted it: all four payload fields are private to their producing module, but a
  `pub` added to one compiles and passes every gate.
  `a_boundary_type_can_be_minted_only_by_its_own_stage` reads the four declarations
  back with `include_str!` — a compile-fail harness would have cost a dev-dependency
  for one assertion.

  **The seam's escape hatch recommended a branch that refuses the same line.** "drop
  `--new-flow` to convert through the current chain" is an unconditional claim about
  the legacy path from a rule that never looked at it — reproduced live with
  `--display-tone none --print-exposure 3`, refused either way. `flow.rs`'s own
  `refusal()` had already learned this lesson one function above; the tail had not
  inherited it. Reworded to say only what the rule inspected, and `using-nc.md` §11's
  console block re-verified by running the binary.

  **Three stale claims elsewhere.** `AcesCgImage::into_linear`'s rustdoc named only
  the named-output split as its consumer, though `working_image::WorkingBuffer::from_aces`
  is now a second one; `pipeline/mod.rs`'s new module map described the new chain
  without noting the seam is still shut; and the entry above under-counts its own
  evidence — the by-hand one-ULP move in `look` also reds
  `leaving_the_chain_preserves_everything_the_encoder_reads`, which was added in the
  first review round, so three tests carry that falsification, not two.

  **`docs/design-spec.md`'s source tree had gone stale again**, and its own closing
  sentence asks for a re-check whenever a module is added. The six new-flow modules
  are listed now, and so are `src/flow.rs` and `pipeline/pixels.rs`, which predate
  this change — that listing and CLAUDE.md's map are the two authoritative module
  lists, and letting them disagree is how the drift it already records starts.

- 2026-09-21: precision round after the targeted re-review — four LOW findings,
  no behaviour change.
  - `a_boundary_type_can_be_minted_only_by_its_own_stage` was one step stronger
    than its assertion: a private field is necessary, not sufficient, since a
    `fn new(buf: WorkingBuffer)` or an `impl From<WorkingBuffer>` inside the
    module would open a second route with the declaration untouched (any
    `pipeline` module can already get a `WorkingBuffer`). It now also counts the
    construction sites — the tuple constructor twice, declaration plus `apply`,
    and never `Self(...)`.
  - `SceneCorrectionParams`'s rustdoc claimed `apply`'s signature accommodates
    "the rest, failure included". Failure is settled; per-stage *report* data is
    not — `sdr::render` and `render_split::display_source` both hand resolved
    parameters back today, and how a filled stage does that is
    `nf-core/report-contract`'s. Scoped the sentence to failure.
  - The seam's tail said the current chain "validates" these settings, but the
    example that motivated the rewrite (`--display-tone none --print-exposure 3`)
    is a *content* refusal from `pipeline::sdr`'s per-pixel range check, not a
    validate-time verdict. Now "checks". `using-nc.md` §11's console block
    re-verified by running the rebuilt binary; the three `tests/pipeline.rs`
    assertions key on "cannot render yet", unchanged.
  - `docs/design-spec.md`'s source tree omitted `src/algo/film_stock/`, a shipped
    runtime module (the `#[cfg(test)]` `curve_probe` beside it is correctly
    omitted). Added, and the whole listing re-diffed against
    `find src -name '*.rs'` — nothing else runtime is missing.

- 2026-09-21 (follow-up, at the user's request): fixed the **source** of the false IR
  claim, `docs/tasks/nf-core/buffer-strategy.md`. It said `--export-ir` and the
  "IR preserved but not used" warning "both depend on [the plane] surviving to the end
  of the run" — false, and the sentence this task's rustdoc had inherited — while
  contradicting itself six lines later with the correct fact (holding the *decoded*
  image for `--export-ir` is what makes `RunProfile::Convert` peak at encode). It also
  carried the 22-of-25 citation, which describes a *returned-fact* defect in
  `film_base`, not a plane dropped at a type boundary.

  Replaced with the real motivation, which is a stronger one for that task: carrying
  the plane is a design commitment with no current consumer, so **nothing today would
  notice if a boundary dropped it** — today's `pipeline::sdr` render drops it outright
  and no test, warning or counter reads zero. The verification section now asks for a
  *positive* assertion that the plane arrives, and marks the two pre-render checks as
  regression checks that cannot stand in for it. Also recorded what this task settled
  provisionally (the plane travels in the stage types; every boundary moves, so the
  identity stages allocate nothing; `ir_verified` still undecided), and narrowed the
  "are identity stages free?" open question to the half that is still open. Verified
  each new claim against the code; the false sentence is now gone repo-wide.

- 2026-09-21 (pre-ship review): one finding, plus a correction to the entry above.

  **The design-spec listing was still incomplete, and the log had claimed otherwise.**
  The entry above records the listing "re-diffed against `find src -name '*.rs'` —
  nothing else runtime is missing". That was an overclaim: `src/pipeline/gain_map/`
  is a directory *beside* `gain_map.rs`, holding a 959-line `iso.rs`
  (`pub(crate) mod iso`, shipped runtime code). The tree listed `gain_map.rs` as a
  plain file while listing `colorimetry/` as a directory in the same block, so the
  submodule was invisible — and CLAUDE.md singles that module out as existing
  precisely because libultrahdr's own ISO serializer is non-conformant, which makes
  it the last one a reader should have to find with `ls`. Now listed as `gain_map/`.
  **The general trap: a `foo.rs` + `foo/` pair reads as one file in a tree listing,
  so "I diffed against `find`" is only true if the diff was by *module*, not by
  path.**

  **Declined, with a reason.** The review noted that the four `apply` functions are
  plain `pub`, so a caller outside `pipeline` could compose the chain by hand rather
  than through `chain::render`, and that `pub(in crate::pipeline)` would cost nothing.
  Left as-is: the type ordering holds either way (nothing outside the producing module
  can mint a boundary type, so a hand-composed chain is still correct-by-construction),
  and `nf-core/buffer-strategy` already flags stage goldens — "a consumed input cannot
  be re-fed to the same stage in a test" — as an open question. Narrowing now would
  pre-empt `nf-verification`'s access to a single stage from outside `pipeline` to buy
  a property the types already guarantee.

  Also fixed: a code span in `buffer-strategy.md` wrapped mid-identifier, which
  CommonMark renders as `working_image:: WorkingBuffer`; and `TASKS.md`'s now-`[x]`
  one-liner said nothing about the seam, so a reader scanning only the checklist could
  infer the chain was CLI-reachable.

## minimal-end-to-end

**Status:** done
**Updated:** 2026-09-22

- 2026-09-19: created with the new-flow plan. Goal: a minimal end-to-end render.
- 2026-09-22: implemented; awaiting review. Scope settled with the user before any
  code: a Display P3 16-bit TIFF destination with the ACEScg → P3 matrix in
  `fit_gamut` (not in the encode), a provisional report block, no sidecar, `roll`
  opened too, and `--export-ir` un-refused.

  **Why the matrix sits in `fit_gamut`.** With every stage an identity the chain's
  exit is still linear ACEScg, so a destination can only declare a profile that
  matches its pixels if something changes primaries. The encode was the cheaper home
  and the wrong one: mapping to a gamut's boundary presupposes being in its primaries,
  so the stage that will map must own the matrix, and putting it in the encode would
  have had to move when `nf-display-stages/fit-gamut` lands. The gamut travels out on
  `DisplayReferredImage` so the encode cannot pick a different one.

  **What the kept flags map onto.** `flow::decode_params` reads `scale`, `offset`, the
  exponential's `gamma` and a `mid-at-base-offset` `d` off the resolved config, and
  uses the decode's own constants where the resolved value can only be a *legacy*
  default (the sigmoid's contrast, `white-at-dmax`). That is safe only because every
  flag and recipe section that could state those values is refused upstream; the
  integration test asserts arrival through the report's `new_flow.decode`, not the exit.

  **Memory, measured** (release, explicit base, peak RSS): 14.45 MP 0.618 GB and
  18.66 MP 0.795 GB, each within 0.1 MB of a legacy u16 `convert` of the same frame, with
  or without `--export-ir`. The pair solves to ~42 B/px + ~10 MB; the model accounts
  38 B/px (0.89–0.94x measured) and estimates 1.20–1.29x. `fixed::decode` clones the IR
  plane beside the decoded image, and the model counts it; moving it would need the
  decoded image consumed, which `--export-ir` reads after the render. The 74.65 MP scan
  the older calibration rows used is no longer in `../nc-assets`.

  **The legacy flow did not move.** Checked against a release build of the base commit
  (`2a15e81`) in a scratch worktree: all 12 presets on both fixtures write byte-identical
  primaries, and the sidecars differ only in the commit/dirty stamp.

  **Two traps met on the way.** An "unclamped" assertion on a vector carrying ±inf and
  NaN passed or failed on the infinities rather than on the finite channels — split it
  out onto finite inputs. And a Rec.709 red is *inside* P3, so it cannot witness a
  negative channel after the matrix; a film-RGB value outside the cube (which the
  unclamped decode can produce) does.
- 2026-09-22: `/code-review` pass, eight of ten findings applied. The one behaviour
  bug: a `--new-flow` run replacing an image a legacy run wrote left that run's
  sidecar beside it, describing a picture that no longer exists — it is now removed
  after the commit, only when it is an nc `{meta, params}` envelope, and reported in
  `new_flow.removed_sidecar`. The stage list moved into `ChainParams::applied` beside
  `chain::render`; `fixed::DecodeReport` serializes directly; the recipe is not
  serialized under the flag; `encode_u16` returns its BigTIFF decision instead of a
  second predictor. **Declined, with reasons:** dropping `pipeline_version` under the
  flag — it labels the build's default render, a non-default legacy config shares it
  too, and `nctool review` treats it as build identity, so a mixed matrix would read as
  two binaries; moving the destination encode out of `color` (a three-line shared
  profile pair, and the new flow keeps lcms2); and sharing one 3×3 between `fit_gamut`
  and its test oracle, which would make the oracle vacuous.
- 2026-09-22: done. Ship review (Codex + `ship:diff-reviewer`) found two holes in the
  stale-sidecar removal, both fixed with regression tests: it keyed on the envelope's
  key names alone, so `{"meta":null,"params":null}` qualified — it now requires the
  identity every sidecar stamps (`nc_version`, `pipeline_version`, `target`) — and it
  deleted a sidecar the run had just loaded as its own `--params` recipe (a legacy
  sidecar with the refused sections stripped is exactly that). Files the run read
  (`--params`, roll's `--frames`) are now threaded into the frame and never removed; a
  failed removal after the image is committed is a warning, not exit 5. Verified: all
  CI gates, legacy output byte-identical to `2a15e81` on all 12 presets, memory
  measured on two frame sizes. **For dependent tasks:** `nf-verification/*` can build
  on the `new_flow` report block and `RunProfile::NewFlowSdrTiff`; `nf-core/subcommands`
  inherits roll under the flag already working; `nf-destinations/preset-set` extends
  `cli::OutputTarget` and `fit_gamut::DestinationGamut`.

## knob-availability-audit

**Status:** done
**Updated:** 2026-09-22

- 2026-09-19: created with the new-flow plan. Goal: audit every knob against the new flow.
- 2026-09-22: scope settled with the user, and the remedy sweep run. No code yet —
  the table half waits on `nf-reconstruction/fixed-decode`, which is finished but
  unmerged (see below).

  **What this task decides: availability, not values.** Which knob the new flow
  accepts, refuses by resolved value, or refuses by flag presence — and whether each
  surviving remedy is still followable. `d`, `scale` and `gamma` belong to
  `nf-reconstruction/anchor-rule` and `nf-calibration/*`; nothing here picks a number.

  **The inventory lives in the tables, not in a document** (user's call, 2026-09-22).
  A hand-kept markdown table of the knob surface, sitting beside a code table that
  lists only the refusals, rots with every gate green; and a generated-and-diffed
  report (the `colorimetry::audit` precedent) was judged more machinery than this
  earns. What makes the code table an *inventory*
  rather than a list of refusals is the part still to land: rows for the **accepted**
  knobs too, plus an exhaustiveness test that reads the flag surface back out of
  `cli.rs` and the recipe surface out of a serialized `ResolvedConfig`, so a knob with
  no verdict reds the gate. Without accepted rows, "considered and kept" and "nobody
  looked" are the same state — which is the silent failure the goal names.

  **This task will build on `fixed-decode`'s model rather than the other way round**
  (CLAUDE.md's rule for concurrent work). That task finished the reconstruction half
  in a parallel session while this one was being planned — finished, not merged, so
  nothing is rebased yet; its diff was read in its worktree. Two of its calls are
  better than the ones drafted here:
  - *`--no-d-max` is refused, not accepted.* The draft had it accepted as an identity
    value — it resolves `DmaxSource::None`, which is what the new flow wants. But an
    identity value is one that asks nothing **of a knob this flow has**, and the fixed
    decode has no reference density at all, so "resolve it from nowhere" is a
    statement about a quantity that does not exist here.

    **Two readings of "no Dmax" land on two different flags, and the refusal wording
    must not conflate them** (raised by the user, 2026-09-22). "I have no leader data,
    use a sensible constant" is `DmaxSource::Fixed` — `NOMINAL_DMAX` 1.3, and the
    *default*, so it is not a flag anyone types. "Use no anchor at all" is
    `DmaxSource::None` / `--no-d-max`, scene-referred output with the base at 1.0.
    Only the second is what `--no-d-max` spells. Neither survives the new flow, and
    neither needs to: `mid-at-base-offset` pins mid-grey a fixed density above the
    **film base**, which every scan carries, so the no-leader case is not a state the
    decode has to be told about — it is the state it is always in. The `--no-d-max`
    capability is not lost either: on a straight line the anchor factors out as a pure
    gain, so the scene-referred *shape* is unchanged.

    **What this does change is the remedy.** `fixed-decode`'s shared `DMAX_REASON`
    reads "nothing in the new flow resolves one; a `Dmax` measured from a leader is
    film saturation…", which a user *without* a leader can read as "you need leader
    data to proceed". Every clause is correct and the conclusion it invites is not.
    The fix is one added fact, which also keeps `instead: None` honest: nothing needs
    stating, because `mid-at-base-offset` measures from the **film base**, which every
    scan carries.

    **Ownership asked, not assumed, and settled** (2026-09-22). The string is
    `fixed-decode`'s, in a tree that has not merged, so editing it here would have
    risked two sessions reasoning about one sentence separately and meeting in a
    conflict. That session took it: the clause ships in its PR, and this task does not
    touch the string. The reasoning is recorded here because it came from this side.
  - *The recipe half closes from the stage, not from a value rule.* A stage that owns
    its parameters reads no resolved section, so the section is refused whole and the
    question "what does a knob the user never typed earn?" does not arise. That
    generalises to this task's half: the four new-flow stages carry their own
    `Params`, so the new flow reads nothing from `print.*` either.

  **Two remedies are reachable under `--new-flow` with only accepted flags, and both
  name a knob it refuses.** Reproduced against the binary (both with
  `--film-base 0.9,0.55,0.42`, which is accepted):
  - `--new-flow --density-gamma 2.5` -> `merge` refuses with "its mid-density slope is
    `--sigmoid-contrast` (or pass `--density-curve exponential`)". The first remedy is
    refused by the flow gate, the second works — followable but half dead.

    **Fixed by symmetry, not by reordering, and not flow-conditionally.** A
    flow-conditional message was rejected: the window closes when the new flow's
    default curve moves, and a message that has to know the flow to be right is the
    coupling this module exists to avoid. Reordering so the always-working option
    leads was proposed and is *not* free either — on the legacy flow it promotes the
    more invasive remedy (switch your curve) over the one that keeps the user where
    they are (`--sigmoid-contrast`), so it trades a dead lead on one flow for a worse
    lead on the other. What costs nothing on both is dropping the ranking: today the
    sentence presents `--sigmoid-contrast` as *the* answer and parenthesises the
    alternative. Stating the two as equals needs no knowledge of the flow, reads the
    same on legacy, and leaves neither flow with a dead lead.

    **Closing condition, stated so it cannot outlive its window silently:** the dead
    half disappears when the new flow's resolved default curve stops being the
    sigmoid, which is `nf-core/minimal-end-to-end`'s wiring of
    `algo::fixed::DecodeParams`. After that the arm is unreachable under `--new-flow`
    and the symmetry is simply better prose.
  - `--new-flow --density-curve exponential --density-gamma 2e-39 --anchor-mid-offset
    0.62` -> the anchor guard refuses with "Use a photographic slope, or
    `--anchor-white-at-reference`, which needs no such division". Every flag in that
    line is one the new flow accepts, and the remedy names one of the three
    placements it refuses. This is the fifth-instance shape CLAUDE.md records: a
    remedy asserting something its own rule never inspected. Independently reproduced
    by the `fixed-decode` session, which left it here.

    **It is the one finding that gets *worse* with time**, which is why it earns the
    care. The other two dissolve when the new flow's default curve moves; this one
    survives `nf-core/default-flip`, after which `nf-retire/dmax-machinery` deletes
    the placement outright and the remedy names a flag that no longer exists.

    **Fixed by saying only what the rule inspected — not by threading `Flow` into
    it.** A flow-aware guard was proposed and declined: no other rule in `validate`
    carries the flow, and gating the clause would leave `nf-retire/dmax-machinery` a
    conditional to unpick. The message has two halves and only one is faulty. The
    *explanation* ("Every placement but `--anchor-white-at-reference` divides by the
    slope, and that quotient overflows f32 for a very small slope") is a fact about
    the arithmetic — true on both flows and after the retirement — and tells a user
    which placements skip the division, so it stays verbatim. The *advice* is where
    the rule recommends a flag whose availability it never checked; dropping that
    clause leaves "Use a photographic slope", correct on every flow and at every
    point in the migration. `flow.rs`'s own `refusal()` learned the same lesson one
    function above: word the escape hatch so it survives the branch you did not look
    at, rather than teaching the rule which branch it is on.

  **A third one this task will *create*, found while classifying `output.*`.**
  `--new-flow -o out.tif` with no preset is refused by the suffix rule with "with no
  `--output-preset`, Hanten writes `gain-map-hdr`" — advice to pass a flag this task
  is about to refuse. The fix is not a rewording: under `--new-flow` there is no
  resolved destination to check a suffix against, so the rule should not run at all
  there. `nf-core/minimal-end-to-end` owns what replaces it.

  **The classification that follows for this task's half**, to be landed as rows:
  `input.*`, `film_base.*` and `measure.*` are **accepted** — decode, film base and
  the measurement region are shared by both flows, and the seam is taken after them.
  Every `print.*` flag is refused by presence and the recipe `print` section whole,
  each naming the stage that will carry it (`nf-scene-correction/stage` for exposure,
  white balance and the flare half of the black point;
  `nf-scene-correction/levels-knob` for `linear_range`; `nf-display-stages/fit-range`
  for the display tone and its headroom). `output.*` is refused the same way, with
  `legacy` and `custom` earning `Never` (they are the print path) where the rest earn
  `NotYet` (`nf-destinations/preset-set`).

  **A fourth, found by `fixed-decode` and verified here — and it is the worst of
  them.** `--new-flow --density-curve exponential --sigmoid-toe 0` (and the same with
  `--sigmoid-shoulder 0`) is refused by `merge` with "…the resolved curve is
  exponential; pass `--density-curve sigmoid` (its slope analogue for exponential is
  `--density-gamma`)". Both flags on that line are accepted — the knee rows key on a
  **non-zero** value, deliberately, so that a zeroed knee stays a usable flags-win
  reset. Unlike the other three this remedy is not half dead but **wholly** dead: the
  parenthetical is an aside about slopes, not a second remedy, and adding
  `--density-gamma 2` leaves the error unchanged (verified). The only action that
  works under `--new-flow` — drop the knee flag — is the one action the message never
  states, while the one it does state is refused. On the legacy flow the named remedy
  works (verified), so this is a `--new-flow`-only defect and the fix is to state the
  escape rather than to gate on the flow.

  **So the sweep's bound was right and its walk was not.** The bound — "only rules
  keyed on an **accepted** knob's value can fire" — holds. What the first pass missed
  is that the accepted set contains more than the accepted *knobs*: a refused knob's
  **accepted identity value** is in it too, and `--sigmoid-toe 0` is exactly that.
  Enumerating knobs rather than reachable values is how a bound that is correct
  produces a list that is short. Corrected walk, adding the identity values the
  refusal rows leave accepted (`--sigmoid-toe 0`, `--sigmoid-shoulder 0`,
  `--balance-range` / `--auto-balance-range` with both balances zero): four findings.

  **Under `--new-flow` every rule whose condition needs a refused flag is
  unreachable** (the presence gate runs before `merge`), and a recipe
  `reconstruction` section is refused whole — so the rules that can fire are those
  keyed on the accepted set above, plus the output path. Checked and found
  unreachable once this task's `print.*` / `output.*` rows land: the
  display-tone-headroom presence rule, `--linear-range`'s span-overflow check, the
  `--display-tone none` combination rule, the atomicity rule and the `--out-depth`
  presence rule — each needs a print or output flag the presence gate refuses, or a
  recipe `print` / `output` section the section refusal blocks. The control: a
  `--new-flow` run using only accepted flags and a `.jpg` path reaches the seam at
  exit 4, so no rule fires on the default config.
  Checked and found unreachable rather than wrong: the characteristic-curve arms for
  the `--anchor-*` and `--film-stock` families, `--sigmoid-contrast`'s upper bound,
  the `film-master` auto-anchor refusal and the `--auto-d-max` warning — each needs a
  flag the gate refuses first, or a recipe value the section refusal blocks.

  **Ownership split with `fixed-decode`, agreed both ways** (2026-09-22): the
  `DMAX_REASON` clause and the `flow.rs` end of the `--sigmoid-contrast` loop ship in
  its PR; `merge`'s `--density-gamma` arm, the knee arms, the anchor guard and the
  suffix rule are this task's. That session also owns the loop it found in the other
  direction — `--sigmoid-contrast`'s refusal points at `--density-gamma`, which
  `merge` then refuses by naming `--sigmoid-contrast` again.

  **One claim not to inherit.** That session's first edit to this task's file said the
  `new-flow-flag` worked instance was *closed*; its own review round found a `roll`
  **per-frame overlay** stating `reconstruction` is still not refused — the raw-JSON
  witness runs on `convert` and on roll's *shared* recipe only, while overlays are
  JSON-merged with no probe. Unreachable while `roll` refuses at the seam, and live
  the moment `nf-core/minimal-end-to-end` removes that guard; `nf-core/subcommands`
  owns it. The qualified wording ships in its PR, so this task should read that file
  after the merge rather than the version drafted before it.

  **Blocked, and deliberately not worked around.** `fixed-decode` is finished but
  uncommitted in a sibling worktree, so there is no base to build the table on;
  writing this half against `main` would mean re-deriving a competing classification
  of the reconstruction knobs and resolving it as a conflict later, which is exactly
  how a re-port drops what nothing references. Waiting for that PR to merge (user's
  call). The sweep above needed none of it.

- 2026-09-22 (shipped, on `fixed-decode`'s merged base): the inventory is complete,
  and three remedies that named a knob this flow refuses are fixed.

  **The tables became an inventory.** `FlagEntry` gained `covers` (the flags a row
  classifies), a `#[cfg(test)]` `KEPT_FLAGS` table records every flag the new flow
  *accepts* and why, and `every_convert_flag_is_classified` reads the flag surface
  back out of `cli.rs` — parsing the `#[arg(...)]` declarations of `ConvertArgs` and
  each group it flattens, the way `stage-skeleton`'s minting test reads the boundary
  types with `include_str!`. Both directions are checked: a flag no row covers, and a
  row covering a flag that no longer exists. Falsified both ways before being trusted
  (blanking one row's `covers`; adding a `--probe-knob` to `cli.rs`), because it
  passed on the first run, which is when a test is most likely vacuous.
  `every_recipe_section_is_classified` does the same for sections, off a serialized
  `ResolvedConfig`.

  **`print.*` and `output.*` are refused the way the decode's section is**, which is
  `fixed-decode`'s pattern rather than the value rules this task had planned: the four
  new-flow stages carry their own params and the chain resolves no destination, so
  both sections joined `reconstruction` in `flow::UNREAD_RECIPE_SECTIONS` and
  `reject_recipe_reconstruction` generalised to `reject_recipe_sections`. Twelve flag
  rows cover the other provenance.

  **The corollary that decided every print verdict.** Once a section is refused whole,
  an **identity value earns no exemption**: the tiebreaker spares one so a flag can
  clear what a recipe pinned, and there is no longer a recipe value to clear. So
  `--white-balance 1,1,1` and `--highlight-compress 0` are refused although they
  resolve the documented defaults and render byte-identically. This also falsified two
  rationales that shipped with the decode — the knee row's "clearing a recipe's knee is
  how one recipe gets re-used on the new chain" and `--density-curve`'s "keeps the
  flags-win reset usable on a recipe that pinned a sigmoid" — both citing a reset the
  section refusal in the same PR had already made impossible. The *behaviour* is
  right for an independent reason (each names what the flow already does), so only the
  prose changed. **A rationale can be falsified by a change that leaves its code
  correct**, and no gate reads prose.

  **Three remedies fixed, each verified by running the binary before and after.**
  - The **anchor guard** (`cli.rs`) ended "Use a photographic slope, or
    `--anchor-white-at-reference`, which needs no such division" — a placement the
    rule never checked was available, reachable on a line built entirely from accepted
    flags (`--density-curve exponential --density-gamma 2e-39 --anchor-mid-offset
    0.62`). Fixed by splitting the message: the *explanation* still names the flag,
    which is a fact about the arithmetic, and the *advice* stops recommending it. A
    `Flow`-aware guard was proposed and declined — no other `validate` rule carries the
    flow, and the flag is **deleted** at `nf-retire/dmax-machinery`, so it should not be
    recommended on any flow rather than hidden under one.
  - `merge`'s **`--density-gamma`** arm ranked `--sigmoid-contrast` as the answer and
    parenthesised `--density-curve exponential`; under `--new-flow` the ranked one is
    refused. Reordering was considered and rejected — on the legacy chain
    `--sigmoid-contrast` keeps the user on the curve they resolved, so promoting the
    switch trades a dead lead on one flow for a more invasive one on the other. The
    *ranking* is dropped instead, which costs neither and needs no knowledge of the
    flow. Closes when the new flow's default curve moves
    (`nf-core/minimal-end-to-end`).
  - The **output suffix rule** stands down under `--new-flow` entirely. It blamed a
    preset nobody selected and pointed at `--output-preset`, which this task refuses.
    Not a rewording: with no destination resolved there is nothing for a suffix to
    match, so `-o` is taken as typed at both sites that judged it (`validate_convert`
    and `run_convert`'s own resolution). A legacy control pins that the rule still
    fires there.

  **The fourth finding was closed by `fixed-decode` instead**, in its review round:
  `--sigmoid-toe 0` beside a knee-less curve now trips the flag row rather than
  `merge`'s dead remedy, via a narrow conjunction that keeps the zero knee accepted
  on its own. Verified both branches here rather than taking the report.

  **What tripped the tests, and why it is the right kind of breakage.** Seven
  integration tests failed the moment `--output-preset` became refused — every
  `--new-flow` test passed `--output-preset legacy` so its `.tif` path would be
  accepted. Two of them (`new_flow_refuses_every_knob_the_fixed_decode_strands` and
  its sibling) were passing only because the reconstruction rows sit above the output
  rows, so the refusal they asserted arrived first: a test asserting "X is refused"
  whose command line also carries a refused Y is correct only by row order. Dropping
  the preset from every `--new-flow` invocation made them assert what they mean.

  **Process note.** Reverting a temporary falsification probe with
  `git checkout src/cli.rs` also discarded unrelated uncommitted work in that file.
  Back up the file instead; `git checkout <path>` has no notion of "just my probe".

  **Docs.** `using-nc.md` §11 re-verified by running the binary (its seam example
  carried the now-refused preset), gaining the print/output table, the
  no-identity-exemption note and the suffix-rule change; `--new-flow`'s `--help` text,
  which claimed the inventory was "still being assembled"; CLAUDE.md's three-provenance
  paragraph, which named the renamed function. `cargo doc` is back at the documented
  16-link baseline — the rename left one stale intra-doc link and two more pointed at
  `#[cfg(test)]` items, which rustdoc cannot resolve.

- 2026-09-22 (review round): eight findings from the two-engine loop, all real, all
  fixed. Codex found nothing; every one below came from the local reviewer.

  **Two of the eight were defects this change itself introduced**, which is the part
  worth remembering.
  - *The suffix stand-down invented a collision.* With the output path taken verbatim,
    `encode::sidecar_path` appends `.json` to the **stem**, so `-o out --report-file
    out.json --new-flow` was refused for colliding with a sidecar that exists on
    neither chain — and the refusal pre-empted the seam, replacing "cannot render yet"
    with a wrong diagnosis. Only the *sidecar* entry stands down now; every other
    write target is a real path on both chains, so an input-clobbering `--report-file`
    is still refused (pinned, with the input's size read back).
  - *The stand-down was applied to `convert` only.* `roll`'s planner still judged a
    manifest's explicit `output` against `gain-map-hdr` — and on `roll` that is
    sharper than on `convert`, because its only way to change the preset is the
    recipe `output` section this same change refuses whole. Stood down in
    `resolve_frame_output` too; the *derived* name needs no guard, being correct by
    construction and never judged.

  **`--export-ir` was classified "kept" on a claim the code contradicts.** The row
  said it is "written from the decoded image before any render, so it is indifferent
  to which chain follows". It reads the decoded image, but it is **staged after**
  `stages::render` — past the seam — and takes its bit depth from
  `cfg.output.depth()`, i.e. from `output.preset`, a section this change refuses. So
  `--export-ir --new-flow` was accepted and wrote nothing: the accepted-and-ignored
  state the audit exists to eliminate, recorded by the inventory as its opposite.
  Now a `VALUE_ENTRIES` row, which covers the recipe key in the same rule — and
  `input` is documented as read *apart from* `export_ir`, rather than widening the
  section refusal and taking `--input-transfer` down with it. **The lesson: "reads the
  decoded image" and "runs before the render" are different claims, and the memory
  model already said which one holds** (`RunProfile::Convert` peaks at encode
  *because* both images are live).

  **`--highlight-compress` was attributed to the wrong tone.** The refusal called it
  the knee width of "`shoulder` and `none`"; `DisplayTone::resolve` *refuses* a
  non-default value under `none` ("applies no shoulder to place"), so the knee is
  `shoulder`'s alone. The pairing is `bounds_sdr_output`'s grouping, not knee
  ownership.

  **Three more stale rationales, and the grep lesson repeated.** `FLAG_ENTRIES`'
  rustdoc still said "the rest of the surface is not closed — `print.*`, `output.*`
  and `measure.*` are still accepted"; the block comment above the decode rows still
  said "the audit still owns the rest"; and `--measure-inset`'s row called `measure`
  "the one section the new flow still reads" while `READ_RECIPE_SECTIONS` lists three.
  The first two sat ten lines from the new paragraph asserting the opposite. **The
  earlier negation grep missed them because it searched for phrasings** ("still being
  assembled", "the rest is not") **rather than for the claim's meaning** — the exact
  failure CLAUDE.md records for this check, committed while quoting the rule. The
  re-run grepped the *meaning* across every path and is clean.

  **And the falsified-reset argument came back in a newly written row.** This change
  rewrote that argument out of the knee row and `--density-curve`'s, then reproduced
  it in `--balance-range`'s: `balance_range` serializes under
  `reconstruction.density`, so a recipe stating it is refused whole and there is no
  reset to protect. The behaviour was right on its other clause; only the sentence
  was wrong. Two comments justifying an `instead` against merge's *ranked* remedies
  were stale for the same reason — this change dropped that ranking — and
  `render_not_implemented`'s doc cited `--display-tone none --print-exposure 3` as a
  line that reaches the seam, which both new rows now refuse by presence.

  **CLAUDE.md's tiebreaker paragraph now carries the exception.** It named
  `--bigtiff auto`, `--highlight-compress 0` and `--display-tone shoulder` as
  identity values that must not be rejected, and `--new-flow` rejects all three. The
  rule reads "spare an identity value where a recipe could have set the knob"; the
  paragraph says so, so a reader landing there does not read the new rows as a
  violation.

  **Two soft spots the reviewer raised and I left.** `every_convert_flag_is_classified`
  scans only `ConvertArgs`' own body for `#[command(flatten)]`, so a *nested* flatten
  inside one of the ten groups would go unscanned — none exists, and recursing would
  add a parser branch with no caller. And `!entry.why.is_empty()` is a weak
  assertion; it exists to stop a row being added with no recorded reason, which is
  all it claims.

  Three regression tests added, each falsified by reverting its fix.

## default-flip

**Status:** done
**Updated:** 2026-09-27

- 2026-09-19: created with the new-flow plan. Goal: flip the default to the new flow.
- 2026-09-26: two dependencies added (user). `nf-destinations/gain-map-destination`:
  the product default is a gain-map JPEG, and flipping before the new chain can write
  one would remove it. `nf-calibration/roll-white-rule`: the last planned move of the
  default render (it follows `parametric-operator`'s black point), so the flip records
  the default that stays rather than one about to change.
- 2026-09-27: started. Scope settled with the user: this task also **deletes the
  current chain** (nothing else owns it, and an unreachable chain fails `clippy -D
  warnings`), in one PR; it flips before `report-contract` and `subcommands`, recording
  what the default run loses meanwhile; the default destination is the new chain's axis
  defaults (SDR Display P3 TIFF, not `gain-map-hdr`); the new drift row hashes the fixed
  decode only. The gap list, with a verdict per ability, is in the task file.

  **Two corrections to the task file.** Its verification asked for the neutrality
  gate's recorded decision, which that gate's own file says does not block the chain
  flip — dropped. And "a recipe carrying `--new-flow` fails to load" could not happen:
  the flag was never a recipe key. The real contract is that a recipe without
  `recipe_version` 2 is refused with a migration message.

- 2026-09-27: done. **The flip.** `--new-flow` is a removed-flag error on `convert`,
  `roll` and `params`; the chain it selected is the only one, and `pipeline_version` 8
  records the new default (an SDR Display P3 16-bit TIFF). The old chain is **deleted,
  not unreachable**: `flow.rs`, `ResolvedConfig`/`merge`/`validate`, `OutputPreset`, the
  print stage, the display-tone/SDR/HDR/gain-map renderers, `stages`, `algo::density`,
  the sidecar writer and `io::ultra_hdr` with `ultrahdr-sys` and its vendored snapshot.
  The removed flags stay hidden and refused (`cli::reject_removed_flags`), and a
  recipe without `"recipe_version": 2` is refused whole.

  **v8's default is v7's `--new-flow` render byte for byte** on nine real frames, one
  per roll (`docs/reports/default-flip.md`); against v7's default every frame renders
  darker, and the estimated peak roughly halves. The v8 fingerprint row's `render`
  hashes the fixed decode over the frozen vectors (the old reconstruction is gone), so
  it moved with the code it covers; `base` did not.

  **Rebased onto `nf-destinations/direct-preset` and `nf-calibration/roll-section`**,
  which landed while this ran: `--rendering` and the roll flags are ordinary flags now,
  the v8 row's `recipe` hash covers their sections, and `--output-preset`'s counterparts
  name one destination under either rendering (`display-p3` is `--gamut display-p3`, the
  gain map `--range hdr --container jpeg`). One consequence worth knowing: the `default`
  rendering's **no-roll warning now fires on every plain `convert`**, so `--strict` at
  defaults needs a measured roll, a stated white balance and contrast, or `--rendering
  direct`. Tests whose subject is not the roll state a neutral one (`MEASURED` in
  `tests/pipeline.rs`).

  **Tooling.** `nctool` reads which flags a build speaks off its banner's
  `pipeline_version` (8+ destinations, older presets), so a review matrix or `compare`
  can mix the reference build with this one; `real-scan-verify` freezes v2 recipes
  stating every axis and expects no sidecar. `docs/using-nc.md` was restructured around
  the one chain (§5 recipes, §6 decode, §7 stages and `measure-roll`, §8 destinations)
  and re-verified against the binary — most of `nf-docs/using-nc`'s rewrite.

  **Gotchas.** A bulk strip of `"--new-flow",` from the tests left every test that
  compared the two chains comparing the new one with itself; each was rewritten or
  deleted by hand, and the stale-sidecar tests now write a pre-flip sidecar themselves
  (`write_stale_sidecar`). `design-spec.md` still describes the preset chain; it carries
  a banner until `nf-docs/design-spec` folds the new design in.


## report-contract

**Status:** done
**Updated:** 2026-09-28

- 2026-09-19: filed after the plan review. Goal: the report and telemetry shape for the new chain.
- 2026-09-28: re-scoped after the flip and implemented. **Decisions (user):** no
  sidecar — the report echoes the recipe and its hash; `new_flow` becomes `chain`,
  same nesting; telemetry times every stage in a fixed field. The hash moved at the
  flip and is not compared across it — `pipeline_version` 8 is the announcement.

  **Timing without a clock in the stages.** `StageClock::time(stage, f)` is generic,
  so `TimingInfo` implements it with `Instant` and tests pass `Untimed`; `chain::render`
  and `render_pair` take it and time each stage, the gain map's second rendition summing
  into the same fields. The film base's one-pixel grade runs through the same clock, so
  its cost lands in `scene_correction` / `look` — microseconds, and not worth a second
  path. `destination` is what `color` used to hold minus the chain: the display curve
  and ICC profile, the Rec.2100 signal, or the gain map's clamp, ratio and encode.

  **One hash.** `Recipe::params_hash` is the only caller of `version::stable_hash` over
  a recipe; `telemetry::params_hash` is gone. The replay test writes the report's
  `recipe` back through `--params` and asserts identical bytes and hash.

  **Roll frames lost their HDR encoder blocks at the flip** (`frame_report_ok` copied
  only the fields it knew); they carry `avif` / `hdr_linear_tiff` / `hdr_coded_tiff`
  again, boxed for `clippy::large_enum_variant`.

  **Checked and left:** `hdr_coded_tiff.interoperability` is constant prose naming the
  AVIF and gain-map destinations, but as advice about alternatives that exist for that
  range, not a claim about the run. `nctool roll`'s `_preset_depth` is not a leftover:
  reference-build rolls still read their depth from the preset. `nctool` reads `chain`
  or `new_flow` (`manifest.chain_block`), since both shapes are `pipeline_version` 8.

  **Found, not fixed:** `roll` emits no telemetry record, and `fit_gamut` returns no
  counts for the report to state (both recorded in the task file).

## recipe-schema

**Status:** done
**Updated:** 2026-09-22

- 2026-09-19: filed after the plan review. Goal: the recipe schema across the flow boundary.
- 2026-09-22: shipped. The new chain reads its own document, `crate::recipe::Recipe`.

  **Decisions (user, 2026-09-22).** A required **document** version,
  `"recipe_version": 2` — not per-section versions, and not a marker that selects the
  chain by itself (`--new-flow` still selects; the marker makes a mismatch loud both
  ways). A **separate type**, not new sections on `ResolvedConfig`, per the migration
  rule. The decode's section keeps the name **`reconstruction`** (the stage's name
  everywhere; the document version tells the two shapes apart). **All four stage
  sections exist now**, as empty objects that refuse any key.

  **What the schema is.** `recipe_version`, `input`, `calibration`, `measure`,
  `reconstruction`, `scene_correction`, `look`, `fit_range`, `fit_gamut`, in chain
  order. `reconstruction` *is* `algo::fixed::DecodeParams` (serde derived on it), so
  there is no mapping between recipe and decode to drift; its keys are `scale`,
  `offset`, `contrast` (the `--density-gamma` flag keeps its spelling —
  `nf-reconstruction/gamma-split` owns renaming it) and `anchor`
  (`{"mid-at-base-offset": d}`, the current chain's spelling of the same rule).
  `calibration` is **its own type** holding the film base only: sharing
  `CalibrationParams` would have written `"dmax": "fixed"` into every new-flow dump,
  a key claiming a reference the decode never reads. No `output` section.

  **How the chains are kept apart.** `recipe::check_body` runs on the raw JSON before
  the typed parse: a missing or wrong version, a `print`/`output` section, or an old
  `reconstruction`/`calibration` key (`schema_version`, `type`, `curve`, `density`,
  `dmax`) is refused by name with where it went. `check_body_without_flag` is the
  reverse. That replaced `flow::reject_recipe_sections` and the
  `UNREAD_RECIPE_SECTIONS`/`READ_RECIPE_SECTIONS` lists, and made two value rows
  unreachable — `calibration.dmax` and `simple` — so both were deleted rather than
  kept vacuous. `every_key_of_the_current_chains_recipe_is_shared_or_diagnosed`
  replaces the section-classification test: a key added to the current chain's
  schema and classified nowhere reds it.

  **Under `--new-flow` the current chain's `merge` no longer runs.** The flags merge
  into the `Recipe` (`recipe::merge`), sharing `cli::merge_shared_sections` with the
  current `merge` so the two cannot resolve a shared flag differently; the stages both
  chains run read `Recipe::to_config`, a projection that fills only the shared
  sections. Two consequences: bare `--density-gamma` now works (the sigmoid default
  that refused it is gone), and `--balance-range` / `--auto-balance-range` — kept
  before because they landed in a section nobody read — had no home, so they are now
  refused. `every_kept_flag_reaches_the_recipe` holds every kept flag to a merge arm.

  **Round-trip.** `--dump-params` is un-refused under the flag and writes the
  `Recipe`; `hanten params --new-flow` prints the default. A dump reloads under the
  flag to byte-identical output (tested through the binary) and is refused without it.

  **Roll.** The shared recipe loads under the flow's schema, and each per-frame overlay
  is checked by `check_body` (overlays may omit the version) and merged onto the
  serialized `Recipe` — which closes the overlay hole the audit left. Only the
  projection is kept per frame; carrying the frame's `Recipe` to the render is noted in
  `nf-core/subcommands`.

- 2026-09-22 (review round): ten findings from `/code-review`; nine fixed, one filed.

  **The "refused by name" promise had two holes.** A current-chain `roll` override was
  never checked for `recipe_version` (only the shared recipe was), and under
  `--new-flow` the keys *both* chains retired (`algorithm`, top-level `density`,
  `film_base`, `input.color`, …) fell through to serde's bare "unknown field": the
  current chain's migration check could not simply be run, because its remedies name
  that chain's homes (`reconstruction.density.scale`). `recipe::RETIRED_KEYS` gives them
  new-chain wording.

  **Ordering.** `recipe::validate` had been placed ahead of `validate_convert`, whose
  first rule — the availability refusal — is documented as outranking every value rule,
  so `--export-ir` beside a bad `--density-scale` diagnosed the scale first. Now one
  composed gate, `cli::validate_new_recipe`, runs the refusal then the decode's rules at
  all three sites (convert, roll's shared recipe, each override). A per-frame failure
  names the frame and only the recipe key (`recipe::KnobNames::KeyOnly`) — `roll`
  accepts no `--density-gamma`.

  **One checker.** `recipe::validate` had re-implemented `algo::fixed::check_params`,
  and the two already disagreed: the stage accepted a zero or negative mid-grey offset
  the recipe gate refused. The rules now live once, in `DecodeParams::check` returning a
  `DecodeFault`, rendered as an internal error by the stage and as a usage error naming
  flag and key by the recipe; the stage now also refuses `d <= 0`, the range
  `mid-at-base-offset` has always had on the current chain.

  **Smaller.** `LoadedRecipe` held both the recipe and its projection; it now holds one
  `RecipeDoc` and projects where needed. Roll serializes the shared document once rather
  than per frame. `every_kept_flag_reaches_the_recipe` asserted only "the recipe
  changed", which a flag wired to the wrong field passes; it now checks each flag's own
  field, and that the check is not already true of the default. Stale references to
  `cli::load_recipe` (now test-only) and a comment claiming the legacy anchor guard
  fires under `--new-flow` were corrected.

  **Filed, not fixed:** the new chain's per-frame overrides of `reconstruction.anchor` /
  `contrast` get no roll-consistency warning. It belongs with carrying the frame's
  recipe to the render, so it is in `nf-core/subcommands`.

- 2026-09-22 (pre-ship review): Codex found nothing; `ship:diff-reviewer` found two
  remedy loops, both the defect CLAUDE.md records as "a remedy must actually work".
  `check_body`'s section and old-key refusals ended "or run without `--new-flow`,
  where it is read" — but they are reachable only in a document stating
  `recipe_version`, which the current chain refuses outright, so the two messages sent
  the user to each other. And `check_body_without_flag` told *any* `recipe_version`
  to pass `--new-flow`, where `1` is refused again. Both remedies now say only what the
  rule checked, and a unit test asserts the loop's wording absent. The anchor-overflow
  remedy also now follows its route (a tiny contrast wants a larger contrast; a huge
  offset wants a smaller offset, where a larger contrast would make it worse).

- 2026-09-22 (rebased onto `nf-core/minimal-end-to-end`, #141): that task shipped the
  render first, and it had solved the decode's knobs its own way — the current chain's
  `merge` still ran under `--new-flow`, and `flow::decode_params` read the fixed
  decode's parameters back off the resolved `reconstruction`. Per CLAUDE.md's rebuild
  rule its render, destination, report and sidecar-cleanup design were taken whole, and
  the recipe re-applied on top: `decode_params` is deleted (the decode reads
  `Recipe::reconstruction`), `convert_frame` takes a `FrameChain` carrying the recipe
  instead of a bare `Flow`, and `recipe::FitGamut` is a section type of its own because
  `FitGamutParams` now carries the destination's gamut and has no `Default`.

  **Two consequences worth knowing.** The deferred "per-frame recipe is dropped" became
  a live bug the moment the seam opened — a roll override of `reconstruction.contrast`
  would have rendered with the shared value — so `PlannedFrame` now carries each frame's
  recipe; a mutation that renders every frame with the shared recipe reds
  `roll_refuses_the_current_chains_keys_from_either_recipe_site`. And #141 had already
  deleted the `--export-ir` value row, so with this task's two unreachable rows gone the
  resolved-value table was empty: `ValueEntry`, `VALUE_ENTRIES`,
  `reject_unavailable_values` and `cli::validate_with_flow` are deleted rather than kept
  vacuous. The availability gate is now flag presence before `merge` plus the recipe
  schema at load.

  **Kept as #141 left it, deliberately:** no sidecar, `recipe` echo or `params_hash`
  under the flag. Those can now carry the `Recipe`, but that changes the report's and
  the sidecar's contract, so it is recorded in `nf-core/report-contract` rather than
  done in a rebase. #141's tests that fed unversioned recipes to `--new-flow` now state
  `recipe_version`, and the "never removes the recipe it read" case uses an enveloped v2
  recipe, since a stripped legacy sidecar no longer loads under the flag.

- 2026-09-28: done, after two review rounds (Codex + `nc-reviewer`, then the user's
  `/code-review`) and a ship review. Beyond the first entry: the film base's
  `ir_separability` and `effective_area` are timed under `film_base` too; telemetry
  reads the hash from the report's `identity` rather than recomputing it;
  `StageKind::ALL` (test-only) ties the stage list to `timing_ms`'s keys; a gain map's
  branch copy counts only toward `total`; `nctool compare diff` gives `null` for a
  timing key one record lacks (schema 8 against 9). Rejected: the echoed recipe
  replays `input.export_ir` (so does every `--dump-params` file). Filed with
  `telemetry/schema-v2`: the clock records a failed stage's time.

  **For dependent tasks.** `telemetry/schema-v2` is unblocked: key failure events and
  the upload projection on `crate::stage::StageKind`, and read the timing shape off
  `TimingInfo`. A new stage is a `StageKind` member, a `TimingInfo` field and a
  `clock.time` call. A roll report still echoes no recipe (its record is the
  `--params` / `--frames` files plus each frame's `overrides` and hash); a roll-level
  echo, or the version-skew warning a replayed echo no longer carries, would be a new
  task.

## subcommands

**Status:** done
**Updated:** 2026-09-28

- 2026-09-19: filed after the plan review. Goal: `roll`, `inspect` and `estimate` under the new chain.
- **2026-09-28 — re-scoped to `roll`'s per-frame overrides.** Checked against the code
  after `default-flip`: `inspect` already reports no `dmax` and every command runs memory
  preflight, so those points are done. `estimate` moved to `core/measure-base`, and
  `nctool roll`'s calibrate step (still `estimate` only, never `measure-roll`) to
  `core/roll-measure-mode`. What stays: the merge onto the serialized shared recipe, the
  roll-fixed warnings for new-chain keys (today only `calibration.film_base` and
  `output` warn; `roll.white_stops` per frame is legitimate, from `reuse.frames`), and
  the error-code check. Now blocks `core/recipe-composition`, which layers on the same
  resolution. The pinned test the old file named,
  `roll_refuses_the_current_chains_keys_from_either_recipe_site`, no longer exists.
- 2026-09-28: done. **Decisions (user):** `rendering` is roll-wide; a warning fires when
  the frame's *resolved* value differs from the shared one and names both (a
  restatement is silent — this replaced the old key-presence probe on
  `calibration.film_base` and `output`); a bad override stays an up-front exit 2, and
  only a frame refused while converting is per-frame (exit 1, siblings written).

  **What landed.** `cli::ROLL_WIDE` is one table of roll-wide values —
  `calibration.film_base`, `roll.white_balance`, each `reconstruction` key, `rendering`,
  `output` — each row comparing typed values (`-0.0` is `0.0`) and printing them only
  for the message. Everything else is frame-local: `roll.white_stops` (a clamp from
  `reuse.frames`) and `input` (it describes each file). `roll_classifies_every_recipe_key`
  fails on a recipe key in neither list and on a row naming no key.

  **One change, one warning.** A frame switching `rendering` also moves its derived
  destination and drops the roll's gains; the `rendering` row names both destinations,
  the `output` row fires only when the frame states another `output`, and the
  white-balance row compares only gains that reach both frame and roll
  (`Recipe::applies_roll_white_balance`: not under `direct`, not on the film master).
  The first version warned three times for that one key.

  **The merge needed no change.** No `recipe_version` 2 default keys off a key's
  absence: unset values serialize as `null`, and unset destination axes are skipped and
  derive from the frame's own `rendering`. Pinned through the binary by
  `a_roll_frame_resolves_as_the_equivalent_convert_and_warns_only_on_roll_wide_values`
  (bytes and `params_hash` against `convert --params <merged>` for a clamp, a
  `rendering` switch and a decode key).

  **Verified:** every CI gate; each new test falsified by breaking its guard; the
  guide's claims by running the binary. **Gotcha:** falsification probes leave the
  built binary stale — rebuild after restoring the source before running it by hand.

  **For `core/recipe-composition`:** the per-frame warnings compare the frame against
  the shared recipe *as resolved*, so once flags layer onto the roll they must be
  applied before `resolve_frames` compares, or a flag-set value will read as a frame
  break.

## buffer-strategy

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: filed after the plan review. Goal: stage seams, buffers and the IR plane.

## release-decoded-image

**Status:** in progress
**Updated:** 2026-10-08

- 2026-10-08: filed from `nf-verification/roll-side-exports`, which retired `--export-ir`,
  the last reader of the decoded image after the decode. Goal: release it early.

### 2026-10-08 — the decode rewrites the scan in place

- **Dropping the scan after `fixed::decode` was not enough.** The decode wrote a new
  buffer and cloned the IR plane, so during the decode the frame held the scan, the new
  RGB buffer and two IR planes: 32 B/px for HDRi, the old render phase's figure. An
  early `drop` would have moved the peak from render to the decode and saved only the
  6 B/px quantize term on a u16 TIFF.
- **So `fixed::decode` takes the `LinearImage` by value** and maps its RGB in place
  (`pixels::map_in_place`); the IR plane moves onto the `FilmRgbImage`. Each output
  sample reads only its own input sample, so the arithmetic, and every bit, is the
  same. It re-validates the buffer lengths through `LinearImage::new`, since the fields
  are public (a wrong IR length used to panic on an `expect`; it is now an error). Callers
  that reuse a scan (test producers, `branch_probe`) clone it.
- **The IR plane's route is unchanged**: it still rides the chain and leaves through
  `into_parts`. Only the clone went; where the plane should travel stays
  `buffer-strategy`'s.
- **Model** (`pipeline/memory.rs`): render is the chain's one buffer, 16 + 12·s B/px;
  encode 22 + 12·s (u16) or 16 + 12·s (f32); the gain map 40 render / 45 encode. With
  nothing sampled, the **float TIFF and `measure-roll` now peak at the decode phase**
  (18 B/px: the f32 image beside the u16 read buffers), which
  `which_phase_peaks_is_per_profile_and_measured_not_assumed` pins.
- **Measured (Linux x86_64, `wait4`, synthetic HDRi frames 5.83 / 18.66 / 74.65 MP,
  explicit base)**, before → after at 74.65 MP: SDR TIFF 2.843 → 1.649 GB, float TIFF
  and film master 2.395 → 1.350 GB, PQ TIFF 2.544 → 1.350 GB, gain map 3.944 →
  2.750 GB — the scan's 16 B/px (14 on the float TIFFs, whose peak is now the decode).
  The new model sits +22.7% (SDR) and +24.4% (float) over the 74.65 MP peaks. The full
  set is in the module doc and `estimate_stays_conservative_against_the_measured_peaks`.
- **Byte-identical**: 42 outputs (seven destinations × three inputs, with
  `--export-film-rgb`) and a `measure-roll` recipe compared with `cmp` against the
  previous build.
- **Gotcha, measuring on Linux:** a `posix_spawn` child's `ru_maxrss` includes the parent's
  peak, so generating a big synthetic scan in the measuring process reports its peak
  for every run (all four TIFFs read 4.215 GB). Generate first, measure in a fresh
  process.
- **Owed before done: the macOS peak on a real 74.65 MP scan** (`scripts/real-scan-verify`
  `resource`, or `/usr/bin/time -l` on an SDR TIFF, a float TIFF and a gain map). Before
  this change macOS peaked ~4 B/px above Linux on every destination; if that holds,
  the float TIFF's estimate sits within about 1% of its macOS peak, and
  `ALLOWANCE_PERCENT` or the decode row may need to move.

### 2026-10-09 — review round

- Two `/code-review` passes. Fixed: the design-spec §8 memory example (regenerated from
  a real half-frame `--base-region` run), `io/decode.rs`'s IR-drop figure (22 → 18
  B/px), the `ALLOWANCE_PERCENT` justification (now says it is unconfirmed on macOS
  against the smaller base), the default-budget doc (rewritten to 34 B/px, 3.05 GB),
  the gain-map allowance arithmetic (~8.5 B/px with the fixed part), a misleading
  `cli.rs` comment, and the peak-phase test, which now pins RGB-only scans too: there
  `U16Tiff`'s decode and encode tie at 18 B/px.
- `measure-roll` measured (Linux, one synthetic frame): 0.112 / 0.343 / 1.351 GB at
  5.83 / 18.66 / 74.65 MP, `accounted` 0.995x at the largest; rows added to the module
  doc and the conservative-estimate test.
- Not changed: `ALLOWANCE_PERCENT` itself (waits on the macOS numbers: whether the
  ~4 B/px is per pixel or proportional decides the fix); the PQ/HLG TIFFs' unused
  quantize term on Linux (on macOS it was real before this change); and `ir_verified`,
  which the decode's output never carried (`working_image.rs`, `buffer-strategy`).

## one-luma-dot

**Status:** done
**Updated:** 2026-09-24

- 2026-09-24: filed from the `path-to-white` review. Goal: one shared `dot` for luminance.
- 2026-09-24: **done.** `pipeline::colorimetry::dot` is the one f32 definition, beside
  the luma vectors its callers already import. The private copies in `sdr`, `hdr`,
  `gain_map` and `fit_range` are deleted, and `look` imports it from `colorimetry`
  instead of from fit range. Decisions:
  - **The current chain's copies were replaced by the import, not left** — the
    change is a deleted function and one `use` line each, and their `mul` helpers
    (matrix × vector) are untouched, so `nf-retire` loses nothing by it.
  - **Inline luma sums went through it too:** `fit_gamut::apply`'s hot loop and the
    test helpers in `chain` and `fit_gamut`. Multiplication commutes exactly and the
    sum stays left to right, so no bit moves.
  - **Deliberately not shared:** `chain_golden`'s written-out sums (they are the
    expected values the stages are checked against, so sharing `dot` would check it
    against itself) and `colorimetry::derive`'s binary64 `dot3` (test-only
    derivation math, independent by design).
  - The inline matrix products in `chain`, `fit_gamut` and `color` are the same kind
    of duplication, one level up; not in this task's scope.

## three-step-pipeline

**Status:** not started
**Updated:** 2026-09-27

- 2026-09-27: filed while re-planning `nf-destinations/direct-preset`, unscheduled. Goal:
  decide whether the chain should be rebuilt as decode → roll → style, now that the
  roll's measurements live in rendering (`docs/design-update.md`, Part 2, "Two
  renderings").
