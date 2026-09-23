# Hanten — nf-core Progress Log

Execution log for the `nf-core` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

The new flow exists and can be selected: the `--new-flow` selector, the stage module tree, a minimal end-to-end render, the knob audit, and the default flip that ends the migration's first half.

The epic was created on 2026-09-19 as part of the new-flow migration plan
(`docs/nf-migration.md`). Landed so far: **`new-flow-flag`** (2026-09-20) — the
`--new-flow` selector on `convert` and `roll`, the availability refusal
(`src/flow.rs`), and the render seam — **`stage-skeleton`** (2026-09-21) — the
chain that seam guards: `pipeline::chain` composing `scene_correction` -> `look` ->
`fit_range` -> `fit_gamut` over `working_image::WorkingBuffer` — and
**`knob-availability-audit`** (2026-09-22), which closed the availability surface.

**What the audit means for every other epic.** Under `--new-flow` the old knobs are
now *refused*, not accepted-and-ignored, so the epic that builds a stage also owns
un-refusing its knobs. Three mechanisms, and which one a knob uses is the thing to
check before adding one back:

- **`flow::UNREAD_RECIPE_SECTIONS`** (`reconstruction`, `print`, `output`) refuses
  those recipe sections *whole*, because each stage carries its own params and the
  chain resolves no destination. A stage that starts reading a section moves it to
  `READ_RECIPE_SECTIONS`; `nf-core/recipe-schema` decides the sections' real shape.
- **A flag row per knob** in `FLAG_ENTRIES`, keyed on presence, naming the task that
  will carry it — never a replacement flag spelling, since that belongs to the task
  that builds the stage. There is deliberately **no renamed-knob mapping table**.
- **`every_convert_flag_is_classified`** reads the flag surface back out of `cli.rs`,
  so a knob added to `ConvertArgs` with no verdict reds the gate. Adding a flag now
  means adding a row (refused or kept) or an allowlist line.

**One corollary that will bite whoever adds a knob back:** once a section is refused
whole, an identity value earns **no** exemption from the presence-vs-value tiebreaker
— the exemption exists so a flag can clear what a recipe pinned, and there is nothing
left to clear. `--white-balance 1,1,1` and `--highlight-compress 0` are refused.

**`nf-core/minimal-end-to-end` made the flag render** (2026-09-22): the fixed decode
(`algo::fixed`, fed by `flow::decode_params`) → NC film RGB v1 → `pipeline::chain` →
one destination, a **Display P3 16-bit TIFF** (`cli::render_new_flow_frame`), on
`convert` and `roll`. What a dependent epic builds on:

- **`fit_gamut` owns the change of primaries** into a `DestinationGamut` carried by
  `FitGamutParams` (no `Default`: the destination states it), and the gamut rides out
  of the chain on `DisplayReferredImage::into_parts`. The encode
  (`color::encode_display_linear`) applies only the transfer, so the embedded profile
  is the shipped `display-p3` preset's, byte for byte. `nf-display-stages/fit-gamut`
  adds the radial map on top of the matrix; with fit range still an identity, bright
  frames clip at the u16 encode (counted, `--strict`-promotable).
- **The suffix rule judges against the new flow's destination** (`cli::OutputTarget`),
  so `-o out` completes to `out.tiff` and `.jpg` is refused naming the flag.
- **No sidecar, no `params_hash`, no `recipe` echo** under the flag, on `convert` or
  `roll`: the resolved recipe describes the legacy chain. The report carries a
  provisional `new_flow` block (decode facts, each stage's `applied`, destination) and
  omits the legacy-chain sections; `nf-core/report-contract` owns the real shape.
- **`--export-ir` renders** (u16, from the decoded image); **`--telemetry*` is
  refused** — its record would name the legacy preset and timing buckets.
- **`roll` refuses an unread section in a per-frame overlay**, not just the shared
  recipe.
- **`RunProfile::NewFlowSdrTiff`** shares `Convert`'s u16 arithmetic, measured on two
  frame sizes. A new-flow buffer added later must move that arm.
- `--density-gamma` still needs `--density-curve exponential` beside it until the new
  flow's default curve moves (`nf-core/default-flip`).

**What a dependent epic needs to know.** Every stage is
`apply(input, &Params) -> Result<Output>`, pure, and an **identity pass** until its
epic fills it — except fit gamut, which already applies the change of primaries (above). The `Result` is there so that filling one needs no re-plumbing: every
stage this chain will host has a fallible counterpart in the shipped code, so the
alternative was changing four signatures, `chain::render` and every test the first
time a stage could refuse a pixel. The stage **order is carried by
the types**, not by the composition function: each stage's input is the previous
one's output, and each boundary type can be minted only inside the module that
produces it, so an out-of-order chain does not compile. Crossing a boundary
**moves** the buffers, so a type per stage costs no allocation. Each stage's
`Params` is an **empty struct** — not an `Option`, because "this stage is off" is
deliberately not expressible — and **is not yet a recipe key**: `nf-core/recipe-schema`
owns the sections and `nf-core/report-contract` the report, and both depend on this
task, so neither surface was pre-empted here. The IR plane rides the whole chain and
leaves it with the image — the exit is `DisplayReferredImage::into_parts`, a
consuming unwrap, and it is the boundary's whole surface. `GradedImage` is the
boundary the SDR/HDR split will split *from*. Nothing about the no-flag path moved.

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

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: flip the default to the new flow.

## report-contract

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: filed after the plan review. Goal: the report and telemetry shape for the new chain.

## recipe-schema

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: filed after the plan review. Goal: the recipe schema across the flow boundary.

## subcommands

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: filed after the plan review. Goal: `roll`, `inspect` and `estimate` under the new chain.

## buffer-strategy

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: filed after the plan review. Goal: stage seams, buffers and the IR plane.
