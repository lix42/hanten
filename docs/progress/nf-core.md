# nc — nf-core Progress Log

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
(`src/flow.rs`), and the render seam — and **`stage-skeleton`** (2026-09-21) — the
chain that seam guards: `pipeline::chain` composing `scene_correction` -> `look` ->
`fit_range` -> `fit_gamut` over `working_image::WorkingBuffer`.

**What a dependent epic needs to know.** Every stage is
`apply(input, &Params) -> Result<Output>`, pure, and an **identity pass** until its
epic fills it. The `Result` is there so that filling one needs no re-plumbing: every
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
leaves it with the image — the exit is `DisplayReferredImage::into_linear`, a
consuming unwrap, and it is the boundary's whole surface. `GradedImage` is the
boundary the SDR/HDR split will split *from*. The seam still returns exit 4 — nothing connects the chain to an output
until `nf-core/minimal-end-to-end`, which also owes the `RunProfile`. Nothing about
the no-flag path moved.

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

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: a minimal end-to-end render.

## knob-availability-audit

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: audit every knob against the new flow.

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
