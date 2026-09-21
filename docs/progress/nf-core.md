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
(`src/flow.rs`), and the render seam, which returns exit 4 until
`stage-skeleton` puts stages behind it. Nothing about the no-flag path moved.

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

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: the new stage module tree.

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
