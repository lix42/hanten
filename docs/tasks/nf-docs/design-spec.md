# Fold the new design into the spec

## Goal

Move the new design out of `docs/design-update.md` and into `docs/design-spec.md`,
which is the sole maintained design source. The update is the source; the spec is
the destination.

## Design

- **Three places carry the weight**: principle 2 in §3, the NC film RGB v1
  contract, and §7's framing of the curves as alternatives. The last is the
  substantive one — they are not alternatives any more. One is the fixed decode;
  the sigmoid and `simple` leave the product, and per-stock inversion becomes an
  optional normalization in rendering.
- **A design update is not a second spec.** Once the spec carries the content,
  Parts 1 and 2 become history. What stays worth keeping in `design-update.md` is
  the evidence: the appendices, the measurements, and Appendix F's record of
  which claims were superseded and why.
- **Scope: intent, not the CLI.** The spec wins on intent and `using-nc.md` wins
  on what the binary accepts; user-facing procedure belongs in the guide.
- **Don't pre-write §9.** Nearly every `nf` task touches a recipe key, so this
  task states the *design* and each knob's key lands with the task that ships it.
  A §9 written ahead of the code is a second source of truth that drifts before
  anyone reads it.
- Can run at any point, and earlier is better: a spec that still describes the
  preset-dispatch chain makes every reader re-derive the design from a changelog.

## How to Verify

- The spec describes the staged chain with no reference to a curve menu, and
  nothing in it contradicts `design-update.md`.
- Every §9 path a shipped knob uses resolves to a real struct field — the
  `deny_unknown_fields` trap makes a stale path a silent rejection.
- Backticked task ids and prose paths cited from the spec still resolve.

## Dependencies

None — can run at any point.
