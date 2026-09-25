# Adobe RGB (1998) as an output gamut

## Goal

Make Adobe RGB (1998) a gamut the new chain can render into: gamut-mapped by fit
gamut, encoded with its own transfer, and tagged with a profile that names it. It is
the one notable omission for a photography tool, and a must-have of the migration
(design-update Part 2). It was reachable only via `--output-profile <path-to-icc>` on
the `legacy` path, which retired on 2026-09-23.

**Scope: the capability, not the selection.** Which destination renders into Adobe
RGB, and how a user picks it, is `nf-destinations/preset-set`'s open question and
`nf-destinations/direct-preset`'s destination. This task does not add a preset name,
a selector or a `--new-flow` flag, and the new flow's one destination stays Display
P3, so no default render moves.

## What is known

- Split out of `output/sdr-preset-followups` on 2026-09-13, and re-scoped on
  2026-09-24. The original framing — an `SdrGamut` arm and a preset on the SDR path
  — predates the migration: `SdrGamut` is the legacy renderer's, and the migration
  rule is to build new stages fresh. The new chain's `fit_gamut::DestinationGamut`
  was written expecting this arm.
- **The colorimetry definition already existed**: `definitions::ADOBE_RGB` was added
  for the analysis tooling (`nctool metrics --space adobe-rgb`).
- Not a one-line addition: fit gamut **gamut-maps** into the destination (the radial
  map), so a new gamut needs pinned artifacts in `pinned.rs`, a `DestinationGamut`
  arm, an ICC profile built from the definition, and gamut-mapping coverage.
  `pipeline/colorimetry/` is the area CLAUDE.md guards most carefully; follow
  `docs/colorimetry-maintenance.md`.
- ProPhoto and ACEScg deliberately stay off the *display* path: they are editing
  spaces, which is what `film-master` and `hdr-linear-tiff` are for.

## Open questions

None left here. Two moved downstream:

- A named destination or a gamut selector — `nf-destinations/preset-set`.
- `RunProfile`: whether the Adobe RGB destination shares `NewFlowSdrTiff` —
  `nf-destinations/direct-preset`, measured as `nf-destinations/memory-profiles`
  requires.

## How to Verify

- The pinned artifacts are audited against the definition and anchored to sources
  that share nothing with it; a render into Adobe RGB is gamut-mapped, not merely
  tagged, and covered by a test.
- `nctool metrics` reads the output with `--space adobe-rgb` and agrees with the
  declared space. Until a destination selects the gamut, this is checked by hand on
  a build that does (`docs/progress/output.md`).

## Dependencies

- [Output presets and guidance](presets.md)
