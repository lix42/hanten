# Rethink the pipeline as decode → roll → style

## Goal

Decide whether the rendering chain should be rebuilt around the three kinds of input a
render now has — the fixed **decode**, the **roll**'s measurements, and the user's
**style** — rather than around today's stages. Not scheduled: recorded so the direction
is not lost, to be taken up after the default flip.

## Design

What is known (`docs/design-update.md`, Part 2, "Two renderings", 2026-09-27):

- The decode was meant to reach the common ground on its own, with rendering left to
  taste. What it could not reach became measured per roll (`hanten measure-roll`: white
  balance and white) and moved into rendering, so today the roll step is spread across
  `scene_correction` and `look`.
- The interim carries the split without moving a stage: a `roll` recipe section
  (`nf-calibration/roll-section`) and `--rendering direct|default`
  (`nf-destinations/direct-preset`).

Open: everything — whether the three steps should be stages, what each owns, and what
it costs to migrate recipes and reports again.

## How to Verify

- A recorded decision, with the design documents updated if the chain changes.

## Dependencies

- [Flip the default to the new flow](default-flip.md) — the rethink follows the migration
- [The roll's measurements as their own recipe section](../nf-calibration/roll-section.md)
  — the interim this would replace ships first
- [Two renderings: `direct` and `default`](../nf-destinations/direct-preset.md) — likewise
