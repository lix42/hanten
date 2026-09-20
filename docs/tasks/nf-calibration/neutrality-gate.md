# The neutrality release gate

## Goal

Hold the default move until neutrality is measured against a known-neutral
reference, a criterion is applied to the number, and the decision is recorded.

**This gate does not block the chain flip.** The chain's defaults move with the
retirements (see `docs/nf-migration.md`), and design-update Part 3 is explicit that
the capture is "a release gate, not a blocker: work continues on visual review until
the frames exist". What this gate holds is **shipping the decode's values as
calibrated** — declaring `scale`, `gamma` and the anchor's `d` good — not the
migration's structure. Nothing should depend on it structurally; it is an exit
criterion, and it is why `docs/TASKS.md` records no dependent for it.

## Design

This is the gate half of
[`algo/split-default-migration`](../algo/split-default-migration.md), carried over
unchanged in substance:

- **The gate is on a different axis from the goal it blocks.** The goal is where
  tone shaping happens; the gate is colour. The residual is not created by the
  migration — it exists today — but the migration makes it more visible, because
  the shoulder it removes was compressing the highlights where the cast lives.
- **A leader cannot settle it**: it cannot separate a non-neutral exposure from a
  scanner-slope error, so it can neither accept nor reject the move. That is why
  the gate points at the calibration frames rather than at the asset set.
- **A measurement alone does not hold the gate.** The capture task correctly
  accepts "we measured it and the residual is still there" as a complete outcome,
  because its job is evidence rather than colour. So this task states what residual
  it is willing to ship under, decides against the measured value, and records the
  decision. **The threshold is unset and this task owns setting it**; today's
  numbers are the only anchor, and `algo/split-default-migration` records them.
- **If the answer is "not acceptable", the remedy is
  [`io/scanner-density-calibration`](../io/scanner-density-calibration.md)'s fit** —
  named as the path, deliberately not as an edge, since the measurement may show
  the residual is tolerable under the decode that ships.

Open: **what "the default move" means under the new flow** — the gate was written
against one migration, and now the default chain, the default decode values and the
default destination all move, so decide whether it fires once at the flip or per
default that touches colour. Also whether the criterion is a single number or
per-roll, given the recorded spread.

## How to Verify

- Neutrality measured on the known-neutral frames, not a leader; the criterion
  stated before the number is read; the verdict recorded in the progress log
  whichever way it goes.
- No default that touches per-channel colour ships without that record.

## Dependencies

- [Tune `scale` and `gamma` by review](scale-gamma-loop.md) — the gate is applied
  to the values that loop settles
- [Capture the calibration frames](../analysis/calibration-frame-capture.md) —
  produces the known-neutral reference the gate names
