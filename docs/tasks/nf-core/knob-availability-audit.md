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
  **A worked instance is already open**, left by `nf-core/new-flow-flag`: the sigmoid
  knees are refused only through `--sigmoid-toe` / `--sigmoid-shoulder`, so the same
  knee stated by a recipe or expanded from `--preset sigmoid-knees` is not refused.
  A value rule cannot simply be added beside the flag rule — the shipped default
  sigmoid *has* knees (`toe: 0.2`), so "refuse a non-zero resolved knee" would refuse
  every `--new-flow` run. The asymmetry runs both ways: typing `--sigmoid-toe 0.2`,
  which resolves today's default, is refused while the identical resolved config with
  no flag reaches the render. Whatever resolves this has to wait for, or move with,
  `nf-reconstruction/fixed-decode`.
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
