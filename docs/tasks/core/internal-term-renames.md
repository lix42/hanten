---
title: Rename internal identifiers to docs/terminology.md's terms
status: todo
deps: [core/value-domain-terminology]
summary: Rust identifiers, constants and the remaining doc comments that contradict docs/terminology.md
---
# Rename internal identifiers to the terminology

## Goal

Make code read like `docs/terminology.md` without touching the public surface.
Known cases: `apply_contrast(slope)` and test helpers that say "contrast" for the
linearization (`BUNDLED_CONTRAST`); the `*_LUMA` constants (luminance vectors);
`headroom_*` for the highlight range; "lift" for fit range's dark floor and HDR ramp;
"rendered stops" vs "look stops"; the four Rust types for the destination gamut.

## Design

Mechanical renames, one area per commit. No pixel or output-byte change: the goldens
and the drift gate must not move.

## How to Verify

- CI gates green; goldens and `version::PIPELINE_FINGERPRINTS` unchanged.
- A grep for each retired word in `src/` comments and identifiers finds only
  intended uses.
