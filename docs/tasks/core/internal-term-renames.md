---
title: Rename internal identifiers to docs/terminology.md's terms
status: doing
deps: [core/value-domain-terminology]
summary: Rust identifiers, constants and the remaining doc comments that contradict docs/terminology.md
---
# Rename internal identifiers to the terminology

## Goal

Make code read like `docs/terminology.md` without touching the public surface.
Known cases: `apply_contrast(slope)` and test helpers that say "contrast" for the
linearization (`BUNDLED_CONTRAST`); the `*_LUMA` constants (luminance vectors);
`headroom_*` for the highlight range; "lift" for fit range's dark floor and HDR ramp;
"rendered stops" vs "look stops"; the four Rust types for the destination gamut;
`recipe::resolve::slope`'s local `colour_slope`, which is the colour base slope, not the
colour slope (linearization × saturation slope).

## Design

Mechanical renames, one area per commit. No pixel or output-byte change: the goldens
and the drift gate must not move.

## How to Verify

- CI gates green; goldens and `version::PIPELINE_FINGERPRINTS` unchanged.
- A grep for each retired word in `src/` comments and identifiers finds only
  intended uses.

## Coordination with the public renames

Agreed 2026-10-15 with `core/rename-decode-look-misnomers`,
`core/rename-fit-range-misnomers` and `core/rename-roll-report-misnomers`. The
decode/look and roll/report tasks rename the Rust identifier that *is* each public
name they change (a serde field or variant, a clap field) and its direct accessors.
The fit-range task changes only the public spellings (`serde(rename)`, clap `long =`);
this task renames every fit-range identifier after it lands and drops those
attributes. Every other identifier is this task's, named to match the new public
names: `--highlight-range` / `highlight_range_stops`, `--dark-base` /
`{"base"}`, `peak_sample`, `film_base_below_mid_stops` / `frame_dark_below_mid_stops`
(so the internal scene-stops `SharedParams.frame_dark_stops` becomes
`frame_dark_scene_stops`), `--linearization`, `reconstruction.anchor_rule`, the look
strings `tone-slope` / `saturation-slope`, `colour_correction`, `frame_exposure`.
