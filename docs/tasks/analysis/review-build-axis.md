# A build axis in the review set

## Goal

Let a review set compare the **same frame and configuration across two builds** of
nc, so a default move can be judged by eye as well as by numbers. Today the matrix
axes are frame and config; both cells of a before/after come from the binary the
generator happens to run.

## Why now

`analysis/comparison-review-tooling` deferred this "until a default actually moves",
with the reason that a build axis needs **render provenance rather than a typed
label**. Two default moves are now filed: `algo/split-default-migration` and
`output/display-p3-default`. Each owes a before/after report, and the visual half of
that review has no tool.

## What is known

- Every nc report and sidecar carries an `identity` block: crate version, git commit,
  dirty flag, target, `pipeline_version`, and the resolved-params hash. That is the
  provenance a build cell should be labelled by, read from the sidecar rather than
  typed into the matrix.
- The generator runs one binary. A build axis means naming a binary per axis value
  (a path, or a git ref the generator builds into a scratch target dir) and recording
  which one produced each cell.
- Byte-identity is per build and architecture, so two builds of the same commit on one
  machine should produce identical cells; the axis exists for *different* commits.
- `nctool compare` already diffs numeric records across builds; this is the visual
  counterpart, not a replacement.

## Open questions

1. Is the axis a third matrix dimension (frame × config × build) or a paired set of two
   matrices with matching keys?
2. Building from a git ref inside the generator versus requiring pre-built binaries.
   The latter is simpler and enough for a before/after.
3. How the app labels a build: short commit plus `pipeline_version`, or a name the
   matrix gives it with the provenance shown on hover.

## How to Verify

- Two binaries over one frame and config yield two cells that toggle in place, each
  labelled from its own sidecar's `identity`, and a cell whose sidecar disagrees with
  the matrix's claimed build is flagged.
- The same binary twice yields byte-identical cells.

## Dependencies

- [Comparison review tooling](comparison-review-tooling.md)

Wanted by [`algo/split-default-migration`](../algo/split-default-migration.md) and
[`output/display-p3-default`](../output/display-p3-default.md) for their before/after
review; not a dependency of either, since their numeric report does not need it.
