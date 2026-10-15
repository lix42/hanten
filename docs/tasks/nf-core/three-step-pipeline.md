---
title: Rethink the pipeline as decode → roll → style
status: done
deps: [nf-core/default-flip, nf-calibration/roll-section, nf-destinations/direct-preset]
tags: [new-flow]
summary: unscheduled, after the default flip
---
# Rethink the pipeline as decode → roll → style

> **From TASKS.md** (migrated 2026-10-14): unscheduled, after the default flip; the interim is a `roll` section and `--rendering`
>
> **Dependency note** (migrated 2026-10-14): filed 2026-09-27, unscheduled: the interim design (the `roll` section and `--rendering`) ships first, and the default flips

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

**Decided 2026-10-14** (progress log): the stages stay defined by operation; the decode
supplies the image, and roll and style are combined into their params in
`Recipe::shared_params`. Follow-up:
`nf-core/resolver-module`.

## How to Verify

- A recorded decision, with the design documents updated if the chain changes.

## Dependencies

- [Flip the default to the new flow](default-flip.md) — the rethink follows the migration
- [The roll's measurements as their own recipe section](../nf-calibration/roll-section.md)
  — the interim this would replace ships first
- [Two renderings: `direct` and `default`](../nf-destinations/direct-preset.md) — likewise
