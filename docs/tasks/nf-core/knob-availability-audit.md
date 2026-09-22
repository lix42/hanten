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

## Open questions

- **Renamed, not removed.** Several knobs have a new-flow counterpart under a
  different name (the `print.*` prefix, `linear_range`). Does the generic refusal name
  the replacement, and who owns the mapping table?
- **A knob the user never typed.** Refusing a flag someone passed is right; refusing
  because a default put them somewhere with no counterpart is not obviously right.
  Accepted-and-ignored is not an option, so each such knob needs a decision.
  **The worked instance left by `nf-core/new-flow-flag` is closed**, and how it closed
  is the pattern to reuse: a value rule could not be added beside the flag rule — the
  shipped default sigmoid *has* knees (`toe: 0.2`), so "refuse a non-zero resolved
  knee" would have refused every `--new-flow` run. `nf-reconstruction/fixed-decode`
  closed it from the other end instead, by giving the new flow its own decode
  parameters so that it reads no resolved `reconstruction` at all; a recipe stating
  that section is then refused whole, and `--preset` by presence. Where a stage owns
  its parameters, "what does a knob the user never typed earn?" stops being a
  question. Where it does not — `print.*`, `output.*`, `measure.*` — it remains this
  task's.

  **One hole is left, and it is a `roll` one.** `flow::reject_recipe_reconstruction`
  runs on `convert`'s recipe and on `roll`'s *shared* one, but a **per-frame overlay**
  is JSON-merged onto that shared config with no such witness, and the value table
  matches only `Reconstruction::Simple` — so a frame whose overlay states
  `reconstruction` is accepted and then read by nothing. It is unreachable today
  (`roll` refuses at the render seam before the per-frame loop runs) and goes live
  exactly when `nf-core/minimal-end-to-end` lifts that guard. Owner:
  `nf-core/subcommands`.
- **A value rule cannot outrank `merge`.** It has nothing to read until `merge` has
  resolved a value, so any command line `merge` itself refuses is diagnosed by the
  legacy chain first. `simple` is listed in *both* tables for that reason (the
  `is_atomic` dual-rule shape). A knob that reaches the new flow only through a
  **recipe** cannot be pre-empted the same way — the recipe's value is not the
  resolved one until the flags have won — so each recipe-reachable knob the audit
  classifies needs a decision about whether that two-step diagnosis is acceptable.
- **How much is testable before the flip?** Everything reachable by passing
  `--new-flow` explicitly is. Whether a test can resolve config *as if* the default
  had moved decides whether this ships a regression net or only a report.

## How to Verify

- A written inventory: every knob and every validation rule whose condition reads a
  new-flow-dependent value, classified by all its inputs, with a recorded decision.
- Every remedy string that would become wrong is fixed; a rule is deleted only if it
  is unreachable from *every* path, not merely from the no-flag one. Where two rules
  name the same knob, the losing rule's wording is asserted **absent**.
- The four CI gates pass.

## Dependencies

- [The `--new-flow` selector](new-flow-flag.md)
