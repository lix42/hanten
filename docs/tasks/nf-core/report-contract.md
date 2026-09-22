# The report and telemetry shape for the new chain

## Goal

Decide what nc's JSON report and telemetry record say about a new-flow run, so the
machine-readable contract moves with the chain deliberately instead of being
inherited one optional section at a time.

## Design

- **Nothing owns this today.** `cli::Report` carries around twenty optional sections
  keyed to the old chain — `reconstruction_result`, `output_render`, `dmax`, the HDR
  blocks. Every one is `Option`, so a new flow that simply omits them parses cleanly
  for any consumer tolerating a missing key. Silence is the failure mode, not a type
  error.
- **Prose that names an operation is a claim about the run.** CLAUDE.md's recorded
  defect is `output_render.content`, which asserted the reference-white-preserving
  shoulder "have all run" for a whole preset, so `--display-tone none` made one
  report contradict itself. A chain whose look stage may be an identity pass
  reproduces that trap at every stage: derive the prose from the resolved chain, or
  state the fact in a field.
- **The telemetry bump is driven by the timing shape, not only by a removed enum
  member.** `TimingInfo`'s buckets are `decode / film_base / algorithm / color /
  encode`, and `algorithm` already lumps reconstruction with the print controls —
  that is the bucket the GPU spike's timing table reads. More named stages means a
  different shape, so decide once whether stages are fixed fields or a map. The
  migration doc's "a schema bump only when an enum member is removed" understates it.
- **Two consumers live outside the crate.** `nctool compare` derives the primary
  artifact's depth from `output_render.encoding` and diffs `timing_ms` bucket by
  bucket; `nctool roll` reads `film_base`, `dmax` and friends to build a calibration.
  A renamed field is their break, not nc's, and their fixtures are part of the change.
- Telemetry stays **operational**: arg-struct only, never a recipe key.

- **The new chain's recipe now exists** (`crate::recipe::Recipe`, `nf-core/recipe-schema`),
  but a `--new-flow` run still writes no sidecar, echoes no `recipe` and reports no
  `params_hash`, because those three were built around the current chain's config.
  The recipe reloads under `--new-flow` (`--dump-params` round-trips byte-for-byte),
  so all three can now carry it; which fields, and how the stale-sidecar cleanup
  changes once a sidecar is written again, are this task's to decide.

## Open questions

- **Does the new flow emit the old report shape while `--new-flow` lives?**
  [The minimal render](minimal-end-to-end.md) leaves this open on purpose — cheap now,
  a migration later — and this task is where it gets decided.
- **Is `identity.params_hash` comparable across the flip?** It hashes the effective
  recipe, whose shape changes, so the hash moves for reasons that are not a pixel
  change — announced, or just allowed to happen?

## How to Verify

- Under each flow the report's sections correspond to the stages that actually ran,
  and no prose names an operation the resolved chain did not perform — proven by
  flipping one knob and asserting the sentence changes.
- `nctool`'s fixtures move in the same change and its suite passes under
  `NCTOOL_REQUIRE_DEPS=1`.
- The telemetry `schema_version` is bumped in the change that moves the timing shape,
  with a serialization test pinning the wire bytes.
- Telemetry on and off produce identical pixels; the four CI gates pass.

## Dependencies

- [The new stage module tree](stage-skeleton.md)
