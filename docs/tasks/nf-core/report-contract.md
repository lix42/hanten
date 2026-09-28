# The report and telemetry shape for the new chain

## Goal

Decide what nc's JSON report and telemetry record say about a run of the one chain, so
the machine-readable contract moves with the chain deliberately instead of being
inherited one optional section at a time.

*Re-scoped after `nf-core/default-flip` (2026-09-27), which deleted the old chain and
its report sections and left a provisional `new_flow` block, no `params_hash`, no recipe
echo, no sidecar, and telemetry schema 8 timed in the old chain's buckets.*

## Decisions (user, 2026-09-28)

- **No sidecar.** The report echoes the resolved `recipe` (what `--dump-params`
  writes, reloadable through `--params`) and `identity.params_hash` hashes those bytes;
  the telemetry record carries the same hash. The pre-flip stale-sidecar cleanup stays.
- **The block is `chain`** (was `new_flow`), same nesting. `nctool` reads either name,
  since builds on both sides of the rename are `pipeline_version` 8.
- **Telemetry times every stage in a fixed field** (`crate::stage::StageKind`), schema 9.
  The chain stages are absent for the film master. `StageKind` is also the report's
  `chain.stages[].stage` and what `telemetry/schema-v2`'s failure events name.
- **`params_hash` moved at the flip and is not comparable across it.** Allowed, not
  announced: `pipeline_version` 8 already marks the boundary, and `nctool compare`
  never pairs frames across pipeline versions.

## Design

- **Prose that names an operation is a claim about the run.** Every per-stage fact is
  a field read off the resolved chain; a stage that moved no pixel reports
  `"identity"` rather than disappearing.
- **The stages stay clock-free.** The orchestrator hands the chain a `StageClock`;
  a stage that runs twice (the gain map's two renditions) sums.
- **Roll frames carry a convert report's hash and HDR encoder blocks**: the per-frame
  `params_hash` of the recipe the frame ran, and the HDR encoder blocks. The roll-level identity carries no
  hash, since frames may differ.
- **Two consumers live outside the crate** (`nctool compare`, `roll`, `review`,
  `metrics`); their fixtures move with the report.

## Known and open

- `roll` emits no telemetry record; `telemetry/schema-v2` is scoped to `convert`.
- `fit_gamut` returns no counts (pixels the radial map moved, or wrote black at
  `Y ≤ 0`), so the report cannot state them.

## How to Verify

- The report's sections correspond to the stages that ran, and no prose names an
  operation the resolved chain did not perform — proven by flipping one knob and
  asserting `chain.stages[].applied` changes.
- The report's `recipe` replays the run to identical bytes and hash.
- `nctool`'s fixtures move in the same change and its suite passes under
  `NCTOOL_REQUIRE_DEPS=1`.
- The telemetry `schema_version` is bumped with a serialization test pinning the wire
  bytes; telemetry on and off produce identical pixels; the CI gates pass.

## Dependencies

- [The new stage module tree](stage-skeleton.md)
