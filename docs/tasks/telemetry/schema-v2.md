# Telemetry event schema v2

## Goal

Evolve the successful-conversion-only local record into typed success and failure
events, so a failed `convert` leaves a record of where and how it failed. The
privacy-minimized upload projection built on these events is
[`telemetry/upload-schema`](upload-schema.md).

The approved field manifest and decisions live in
[`docs/telemetry-strategy.md`](../../telemetry-strategy.md).

## Decisions (2026-09-27)

Taken at task start, before any code; they override the Design below where the
two disagree.

- **Blocked on [`nf-core/report-contract`](../nf-core/report-contract.md).** The
  stage enum and timing fields below are the legacy chain's buckets, which
  `nf-core/default-flip` deletes. The new chain's stage/timing shape is that task's
  decision, so this contract waits for it instead of being built twice.
  *Settled 2026-09-28:* the stage enum is `crate::stage::StageKind` (reuse it; the
  Design's "introduce `StageKind`" is done), and `timing_ms` (schema 9) has one field
  per stage, the four chain stages absent for the film master.
- **Legacy local records are never uploaded.** Neither the projection nor the
  uploader reads any record older than the new local event schema. This removes
  the legacy-projection, `source_schema_version: 1` and known-16-bit legacy-fixture
  requirements below, and amends the strategy note (see its Amendments).

## Decisions (user, 2026-09-28)

- **Split.** This task ships the local events and their emission; the upload
  projection, JSON Schema and corpus are [`telemetry/upload-schema`](upload-schema.md),
  which also records the approved revision of the upload manifest (it answers the
  earlier open questions on `conversion.algorithm`, `output_depth`/`output_mode`,
  exit 6 and schema numbering).
- **The local event carries its `event_id`**, minted when the event is written, so
  every retry of it keeps the same ID and the projection copies it. The uploader no
  longer assigns IDs.
- **clap parse failures are deferred to `telemetry/upload`.** Before parsing, nc
  cannot know whether `--telemetry` was passed, so only persistent consent can
  justify recording them. This task reserves the `parse` stage value; failures after
  parsing (recipe load, validation, target collisions) are recorded now.
- **A failed stage's time counts only toward `total`**, never toward that stage's
  field: a stage field means completed-stage performance.

## Design

- The local event (next `SCHEMA_VERSION`) adds a random 128-bit `event_id` (32
  lowercase hex characters, never reused as an installation/session ID), an event
  discriminator, `outcome.status: success|failure`, a typed error kind and exit
  code, the failed/active stage, elapsed time, and optional completed-stage/image
  context.
- Refactor the orchestrator so a small telemetry attempt/context tracks the active
  stage and completed timing/facts without affecting stage inputs. Emit success
  only after artifacts and strict checks succeed. Emit typed failure events for
  recoverable command failures, including strict-warning promotion, without ever
  passing an error display string into telemetry.
- V1 telemetry supports `convert` only. `roll`, `inspect`, `estimate`, `params`, and
  unknown-subcommand failures must not enter the conversion denominator.
- Automatic collection is conditional on persistent consent (implemented by
  `telemetry/upload`); explicit `--telemetry` remains local per-run collection.
  `--telemetry-file` remains a one-off sink and does not itself imply upload.
- Any wire-shape change bumps the local schema version. Keep builders pure by
  injecting time, ID generation, platform, and other ambient values at the
  orchestration boundary.
- Represent unknown/not-yet-completed fields with `Option`, not zero/false; map
  `NcError`/strict failures without storing messages.

## How to Verify

`cargo test` passes with:

- full/minimal local-event snapshot tests;
- success, decode failure, unsupported input, write failure, usage failure, and
  strict-promotion end-to-end events with correct stage/exit category;
- a failure before decode has no invented image/timing fields;
- other commands emit no event;
- telemetry on/off still produces byte-identical output, and collection failure
  cannot change the command exit code.

## Dependencies

- [Telemetry strategy spike](strategy.md) — fixes the event manifest,
  privacy boundary, and success/failure semantics.
- [The report and telemetry shape for the new chain](../nf-core/report-contract.md)
  — fixes which stages a record times and what it says ran.
