# Flip the default to the new flow

## Goal

`--new-flow` stops existing, because there is no longer a second flow to select.
The chain's defaults have already moved piecewise: each `nf-retire` task that removes
a default component (the sigmoid, the `shoulder` tone, the `Dmax` anchor) flips that
default in the same change and pays its own `pipeline_version` bump. This task is the
last of those, plus the flag's removal and the record of what the default now means.

## Design

- **The flag becomes a removed-flag error**, on the `--algorithm` precedent
  (`cli::reject_removed_flags`): a hidden arg emitting actionable guidance, no alias.
  nc is unreleased, so old recipes get a migration error rather than compatibility.
- **A `pipeline_version` bump with its own `PIPELINE_FINGERPRINTS` row.** Never edit a
  historical row in place — that makes one version label two behaviours. Scope the row
  carefully: the `render` row hashes `reconstruct_and_print`, whose `reconstruct` half
  *is* the decode being kept, while `base` and `recipe` have nothing to do with the
  print path. Retire the print half, not the row (design-update Part 2, Decisions).
- **A before/after report** under `docs/reports/`, measured rather than asserted, and
  `docs/using-nc.md` re-verified **by running the binary** — the guide's contract is
  that it is checked against `nc`, and every staleness sweep has found its own examples
  broken in ways no changelog mentioned.

**This supersedes the flip half of
[`algo/split-default-migration`](../algo/split-default-migration.md).** That task
carried the same obligations for a narrower move (`characteristic-generic` as the
no-flag default) and the same release gate; the gate itself moves to
[the neutrality gate](../nf-calibration/neutrality-gate.md), which owns the threshold
and the decide-either-way rule. Read it first — its "known vs unknown" section is
still accurate about what a default move owes.

## Open questions

- **Does the default destination move in the same bump?** It need not: the container
  default is untouched by the chain's piecewise flips, so `nf-destinations/default-destination`
  can land on its own schedule.
- **What the report's prose claims.** Flipping the default changes what every stage
  *does*; prose naming an operation is a claim about the run and must be derived from
  the resolved chain.
- **Is there anything the old flow could do that nothing new can yet?** Retiring a
  path is a list of abilities the new code may need — but the list has to be produced
  before the flip, not after.

## How to Verify

- A bare `hanten convert` resolves the new chain; `--new-flow` exits 2 with a migration
  message naming what replaced it, and a recipe carrying it fails to load.
- The new fingerprint row exists and the gate is green; no historical row changed.
- The release gate's recorded decision exists — the measured residual *and* the
  judgement made against it, either way.
- `docs/using-nc.md` verified against the binary, and the four CI gates pass.

## Dependencies

- [A minimal end-to-end render](minimal-end-to-end.md)
- [Audit every knob against the new flow](knob-availability-audit.md)
- [Retire the sigmoid and `simple`](../nf-retire/sigmoid-and-simple.md)
- [Retire the `shoulder` and `none` tones](../nf-retire/display-tones.md)
- [Retire the `Dmax` anchor machinery](../nf-retire/dmax-machinery.md)
