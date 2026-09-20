# A benchmark set for the new flow

## Goal

Replace the legacy-pinned fixed comparison set with one that runs the new chain,
so `nctool compare` keeps measuring what nc actually does.

## Design

- **Every case in `scripts/analysis/benchmark.json` states `--output-preset
  legacy` explicitly**, and its own note says why: the cases predate the
  gain-map default and exist to stay comparable against records made before that
  flip. The frozen roll recipes they reuse (`scripts/real-scan-verify/recipes/`)
  name legacy too, so both halves move together.
- **The constraint that froze the set is gone.** Cross-build comparability now
  comes from re-running the tagged reference build, not from keeping legacy cases
  alive in the tree — so the set can be rebuilt on the new chain rather than
  grown beside the old one.
- **Keep the two properties the current set has**: the fixtures set is tiny and
  committed, so `compare` runs on any checkout with no Drive assets, and running
  one build against itself is the determinism / zero-diff check. Coverage should
  follow the new axes — one case per destination, both input formats — rather
  than the old curve menu.
- Retiring the old records is a decision, not a side effect: `benchmark.json`'s
  note says changing the fixed set is `core/conversion-versioning`'s call, so
  record there that the pre-migration records are superseded by the reference
  build.

## How to Verify

- `python -m nctool compare` runs the new set on a fresh checkout with no assets
  and no venv beyond the documented one.
- Two builds of one commit diff to zero on every case; the tagged build and the
  new default differ on the cases that should differ.
- No case names a preset, curve or flag that no longer exists after `nf-retire`.

## Dependencies

- [The frozen reference build](reference-snapshot.md)
- [A minimal end-to-end render](../nf-core/minimal-end-to-end.md)
