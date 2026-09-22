# Audit every knob against the new flow

## Goal

Before the default moves, produce a decision for every existing knob: does the new
flow accept it, reject it by **resolved value**, or reject it by **flag presence** —
and is its remedy still followable. The classification has to exist in writing,
because the failure it prevents is silent: a rule that reads a value the user never
typed starts refusing commands that work today.

## Design

**The organising distinction is presence versus resolved value**, and it has already
cost shipped bugs here. CLAUDE.md records it under output-preset atomicity: an atomic
preset rejects a non-default *resolved value* for `output.depth`, but rejects the
`--out-depth` **flag** by presence, because `--out-depth u16` resolves the documented
default and no value rule can see it. The recorded tiebreaker — two reviewers proposed
widening it and both were wrong — is **reject by presence only when the flag forces
something the branch cannot produce**; an identity value asks for nothing and stays
accepted.

This task **supersedes [the characteristic-default audit](../algo/characteristic-default-audit.md)**,
which asked the same question for a smaller default move. What carries over:

- **Classify by every input a rule's condition reads**, not by which gate it lives in.
  That file's worked example is a rule whose rustdoc calls it presence-keyed while its
  body also matches on the resolved curve — a conjunction, and the dangerous class.
- **Rule ordering is part of the diagnosis** and has shipped wrong four times.
- **Drive tests through the real path** (`merge`, the binary), never the rule in
  isolation — a direct call exercises the rule and never the ordering.

What is new: the new flow refuses unavailable knobs through **one generic rejection**
([the flag task](new-flow-flag.md)), so this audit's product is a *list* feeding that
one message, not a second matrix to keep in step.

## Open questions — settled 2026-09-22

All four were decided as the audit landed; the reasoning is in
[the progress log](../../progress/nf-core.md#knob-availability-audit).

- **Renamed, not removed — who owns the mapping table?** *Nobody, and that is the
  answer.* A `NotYet` names the **task** that will carry the knob, never a flag
  spelling: the spelling belongs to whichever task builds the stage
  (`nf-core/recipe-schema` owns the sections), so writing one here would invent a
  second source of truth for it *and* hand the user advice they cannot act on today.
  `Never` keeps an `instead`, because a replacement that already exists can be named.
- **A knob the user never typed.** Closed from the stage, not from a value rule —
  `nf-reconstruction/fixed-decode` set the pattern and it generalised: the new chain's
  four stages each carry their own params and it resolves no destination, so `print`
  and `output` join `reconstruction` in `flow::UNREAD_RECIPE_SECTIONS` and are refused
  whole. **A corollary worth keeping:** once a section is refused whole, an identity
  value earns no exemption — the tiebreaker spares one to keep the flags-win reset
  usable, and there is no pinned recipe value left to reset. That is why every print
  flag is refused at every value, including `--white-balance 1,1,1`, while the
  decode's zero knee stays accepted on its own merits.
- **A value rule cannot outrank `merge`** — and after the section refusals, no
  `print.*` or `output.*` knob needs one at all: the flag rows and
  `reject_recipe_sections` cover both provenances between them, so the two-step
  diagnosis this question worried about does not arise for them.
- **How much is testable before the flip?** It ships a **regression net**, not a
  report, and needs no "as-if-the-default-had-moved" mechanism. Everything refused is
  refused by passing `--new-flow` explicitly, and every row is driven through the
  binary with a no-flag control at exit 0. What keeps the *inventory* honest is
  `every_convert_flag_is_classified`, which reads the flag surface back out of
  `cli.rs` and the recipe surface out of a serialized `ResolvedConfig`, so a knob
  added later with no verdict reds the gate.

  **One hole is left, and it is a `roll` one.** `flow::reject_recipe_sections`
  runs on `convert`'s recipe and on `roll`'s *shared* one, but a **per-frame overlay**
  is JSON-merged onto that shared config with no such witness, and the value table
  matches only `Reconstruction::Simple` — so a frame whose overlay states one of the
  unread sections is accepted and then read by nothing. It is unreachable today
  (`roll` refuses at the render seam before the per-frame loop runs) and goes live
  exactly when `nf-core/minimal-end-to-end` lifts that guard. Owner:
  `nf-core/subcommands`.

## How to Verify

- A written inventory: every knob and every validation rule whose condition reads a
  new-flow-dependent value, classified by all its inputs, with a recorded decision.
- Every remedy string that would become wrong is fixed; a rule is deleted only if it
  is unreachable from *every* path, not merely from the no-flag one. Where two rules
  name the same knob, the losing rule's wording is asserted **absent**.
- The four CI gates pass.

## Dependencies

- [The `--new-flow` selector](new-flow-flag.md)
