# Retire the `Dmax` anchor machinery

## Goal

Remove the reconstruction anchor and the roll-fixed reference measured from a
leader or reference frame, now that the only anchor rule is reference-free.

## Design

- **What retires**: `DmaxSource`'s whole family (fixed nominal, explicit, the
  per-frame percentile `Auto`, and `none`) with its flags, the provenance
  reporting, and `estimate`'s `--d-max-region` half. `estimate` keeps its
  film-base half.
- **The effective area stays.** It is resolved, warned on and reported on every
  `convert` and `inspect` run, and three live tasks build on it
  (`film-base/holder-masked-measurement`, `film-base/tiling-uniformity-validator`,
  and this epic's own `nf-look/scene-range-mapping`). Retiring it as anchor
  machinery would break all three. What goes is `auto_dmax`'s *use* of it — which
  leaves `measure.inset` without its one consumer, so this task must either give
  the knob a new consumer or retire the knob with it, not leave it
  accepted-and-ignored.
- **The anchor rule that survives, `mid-at-base-offset`, never reads a resolved
  `Dmax`** — that is what keeps a leader's roll-to-roll error out of the render.
  Once it is the only rule, everything that resolves an anchor is dead weight.
- **Measuring a frame's own range is a different capability.** It may return at
  the display stage as an opt-in (`nf-look`'s scene-range mapping), written fresh
  there — its failure mode is the *unbounded* stretch, not the idea. Nothing here
  is preserved in anticipation of it, and it must never become a default: roll
  consistency is the product's central promise. The same holds for the "minimum gap
  below the leader's `Dmax`" guard proposed for a content-referenced white
  ([`anchor-rule`'s handoff](../nf-reconstruction/anchor-rule.md#evidence-handed-to-anchor-comparison)):
  if `nf-calibration/anchor-comparison` adopts such a white, its guard reads a
  measurement written fresh for it, and the `pre-new-flow` tag keeps the old one
  comparable.
- **The memory model counts these rectangles.** The sampled-region term covers
  `--d-max-region` and `estimate --grid`; removing a rectangle changes the
  arithmetic the gate is calibrated against, and nothing tests that model against
  the code.
- Prose goes stale here more than code does — the HDR renderer's error message
  tells users to run `hanten estimate --d-max-region`, and `types.rs` carries long
  rustdoc on why `Auto` was demoted.

## How to Verify

- No flag or recipe key resolves an anchor; a recipe stating `calibration.dmax`
  fails with a migration error.
- `estimate --help` and the guide no longer name the region, and no error
  message recommends a flag that does not exist.
- The memory profiles are re-checked against a measured run rather than assumed
  unchanged.

## Dependencies

- [The anchor rule](../nf-reconstruction/anchor-rule.md)
- [Retire `legacy` and `custom`](legacy-custom.md)
