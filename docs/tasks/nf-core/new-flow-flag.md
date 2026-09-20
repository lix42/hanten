# The `--new-flow` selector

## Goal

Give the migration a switch: a presence flag that selects the new chain, so the new
stages can be built and reviewed against the shipped ones without either path being
half-live. It is **scaffolding with a written expiry** — the flag is removed by
[Flip the default to the new flow](default-flip.md), not kept as a feature.

## Design

- **CLI-only, never a recipe key.** Same class as `--report`, `--telemetry` and
  `--max-memory`: it lives on the arg struct alone, and a recipe naming it is
  rejected as an unknown field (every recipe struct is `deny_unknown_fields`). It is
  not a conversion knob — it selects *which set of knobs exists*.
- **Coarse availability, one message.** A knob with no meaning under the new flow is
  refused by **one generic rejection**, not a per-knob matrix maintained in two
  places. Which knobs fall in that set — and whether each is refused by flag presence
  or by resolved value — is [the audit](knob-availability-audit.md)'s output; this
  task ships the mechanism and the wording.
- **The expiry is part of the design.** When the default flips, the flag becomes a
  removed-flag error on the `--algorithm` precedent: a hidden arg emitting actionable
  guidance, no alias (`cli::reject_removed_flags`, `src/cli.rs`). nc is unreleased, so
  removal is cheap; say so in the flag's own help text.

## Open questions

- **Where the branch is taken.** The orchestrator (`main`/`cli`) is the house answer —
  stages stay pure — but the new chain also has to reach `roll`, and roll resolves per
  frame. Decide whether `roll` accepts it at all during the migration.
- **`--new-flow` or `--pipeline <name>`.** A presence flag is the smaller surface and
  matches the expiry; a named selector would survive a third flow, which nothing
  plans. The migration doc names both spellings.
- **What the generic refusal says.** It must name the knob the user typed and the
  fact that the new flow has no counterpart *yet* versus no counterpart *ever* — those
  are different sentences and the audit decides which knobs get which.

## How to Verify

- `nc convert --new-flow …` resolves the new chain; without it, nothing moves —
  byte-identical output and an untouched `PIPELINE_FINGERPRINTS`.
- A recipe containing a `new_flow` key fails to load with the unknown-field error.
- One knob with no new-flow meaning is refused with the generic message; a control
  run without `--new-flow` accepts the same knob at exit 0.
- `--help` documents the flag as transitional.
- The four CI gates pass.

## Dependencies

None — the first task of the migration.
