# Retire the sigmoid and `simple`

## Goal

Remove the sigmoid curve — its toe and shoulder knees — and `simple`
reconstruction, leaving the fixed decode as the only reconstruction.

## Design

- **Both are decodes of nothing.** The sigmoid is a decode with a rendering fused
  on top; its shoulder is a stage-boundary violation, not a rendering option.
  `simple` (`1 − T/T_base`) is an affine inversion of the scan, not a decode of
  anything a print sees.
- **This is a removal, not a replacement.** The exponential is the sigmoid with
  `toe = shoulder = 0` at the same anchor, bit-exact, so what survives is already
  reachable through the curve being kept.
- **Known cost: `Reconstruction::Simple` is the cheap test fixture** in around a
  dozen unrelated modules (memory, gain map, AVIF, encode, working space and
  others) — it is simply the fastest reconstruction to write into a test that is
  about something else. Pick one cheap new-flow reconstruction and move them all
  to it; a bespoke fixture per module is how a helper ends up with a dozen
  slightly different meanings.
- **`--preset sigmoid-knees` is the migration's visual reference**, and it must
  stay reproducible after this lands. It stays reproducible from the tagged
  build — that is what the reference snapshot is for, and not a reason to keep
  the curve in the tree.
- Recipes naming a removed curve get a migration error; no aliases.

## How to Verify

- `reconstruction.curve` accepts neither, and a recipe naming one fails with a
  message naming the replacement.
- The goldens and probes covering them are deleted rather than silently
  re-pointed at another curve — a probe that quietly changes what it measures
  keeps printing plausible numbers at exit 0.
- The suite runs without them and no module grew its own fixture.

## Dependencies

- [Retire `legacy` and `custom`](legacy-custom.md)
- [Goldens for the new stages](../nf-verification/stage-goldens.md)
- [The fixed, stock-agnostic decode](../nf-reconstruction/fixed-decode.md) — the
  default has to have somewhere to move to
