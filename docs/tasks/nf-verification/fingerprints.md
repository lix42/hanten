# Rebase the drift gate on the new chain

## Goal

Rework `version::PIPELINE_FINGERPRINTS` so its rows describe the new chain rather
than the legacy print path, without losing the property the gate exists for: a
default cannot move inside the fingerprinted stages without the version moving
with it.

## Design

- **Retire the print half, not the row.** The `render` row hashes
  `reconstruct_and_print`; its `reconstruct` half *is* the decode being kept, and
  the `base` (film-base estimation) and `recipe` (default recipe document) rows
  have nothing to do with the print path at all. Only the print half needs
  replacing.
- **Decide where the new `render` hash stops.** Reconstruction alone keeps the
  gate stable while the display stages are still moving, but then no look or
  fit-range default is defended; carrying it through fit range defends them and
  trips on every tuning round. Pick deliberately and say so in
  `PipelineFingerprint`'s rustdoc, which already insists the gate must not be
  described as whole-pipeline coverage.
- **Supersedes `algo/characteristic-fingerprint-vector`.** That row is never
  written, but its analysis carries over intact: the chain makes two libm calls
  (`log10` in `to_density`, `10^` in the curve), a 1-ULP difference in the first
  is amplified into tens of pixel ULPs, and four of the current samples sit too
  close to an f32 rounding boundary to be provably portable. A fingerprint hashes
  raw f32 bits and has **no window**, so a non-portable sample must be designed
  out of the vector rather than bounded.
- **From `stage-goldens`** (`docs/progress/nf-verification.md`, 2026-09-23): a decode
  pixel cannot be *proved* portable to the bit, structurally — its final `powf` may
  land either side on a conforming target — so a hash through the decode rests on
  observed agreement. The measured part is the per-sample window list there: the base
  and above-base samples carry nothing beyond that final call, the dead pixels up to
  37 ULP. Everything downstream of the decode is IEEE-only.
- **Historical rows are history.** Never edit an existing row's `render` or
  `base` — that would make one version label two behaviours. The migration is a
  version bump with a new row, and a bump with no row panics by design.

## How to Verify

- The bumped version's row is computed on both CI targets and agrees; a
  deliberate default change inside the covered stages fails the gate with the
  copy-pasteable message.
- Historical rows and their `behavior` strings are untouched.

## Dependencies

- [A minimal end-to-end render](../nf-core/minimal-end-to-end.md)
