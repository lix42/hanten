---
title: The white rule in HDR
status: done
deps: [nf-calibration/roll-white-rule, nf-display-stages/parametric-shoulder, nf-calibration/roll-exposure, nf-calibration/span-roll-slope]
tags: [new-flow]
summary: the cap and floor stand
done: 2026-10-12
---
# The white rule in HDR

> **From TASKS.md** (migrated 2026-10-14): **done 2026-10-12: the cap and floor stand.** Nothing looked wrong on the gain-map JPEG under `hybrid` or `frame` (28 frames, eleven rolls). How much HDR range is used is set by the headroom and the placement more than the cap or floor. The gamut map is nearly inert on HDR
>
> **Dependency note** (migrated 2026-10-14): filed 2026-09-27: the rule was reviewed on SDR only; `roll-white-rule` handed HDR to `parametric-shoulder`, which kept reinhard and left it unreviewed; after `roll-exposure` (2026-09-29), which sets the level the rule is reviewed at

## Goal

Review the roll's white placement on an HDR rendition, so its values can stop being
provisional. Every round that chose and checked it was judged on SDR renders. Since
2026-10-09 that placement is [`span-roll-slope`](span-roll-slope.md)'s span slope, built
on `roll-white-rule`'s cap and floor.

**Done 2026-10-12: the values stand.** No HDR rendition looked wrong, so nothing moved.
The record is in the progress log.

## Design

The original Design section predated `span-roll-slope` and the hybrid placement. What was
reviewed, at `pipeline_version` 14:

- **What was under review.** The white target is gone (`span-roll-slope`). The cap (+2.0)
  and floor (+1.5) now feed the span slope, the hybrid's slope bound and the clamp. The default
  placement is the hybrid; `--placement frame` is the opt-in alternative. There are eleven
  rolls, not nine.
- **The rendition.** The default `--range hdr` gain-map JPEG in Chrome on an HDR
  display, beside the SDR JPEG of the same grade, under `hybrid` and `frame`. Two or
  three frames per roll (28): one held at the cap, the one using the most HDR range, and
  an ordinary one, where one frame can fill two roles.
- **Measured alongside.** Every frame's HDR luminance and the share of pixels lifted
  above white, inside the effective area. Also the gamut map's share on both branches,
  using a throwaway build with the map turned off.
- **The levers held back for a second round**: the cap, the floor, and the headroom
  (`--display-tone-headroom`). The headroom decides how much of the HDR range is used
  far more than the cap or floor does. No round 2 was needed.

## How to Verify

- A review set on HDR renders across the rolls (clamped and ordinary frames) with
  the rule's contrast, and a recorded verdict: the cap and floor stand or move
  (the target was removed by `span-roll-slope`).
- If a value moves, `measure-roll` and its tests follow it, and the SDR verdicts are
  re-checked.

## Dependencies

- [`measure-roll` places the roll's white](roll-white-rule.md) — the rule under review
- [Does a parametric shoulder beat reinhard?](../nf-display-stages/parametric-shoulder.md)
  — settles the operator the HDR rendition is judged under
- [A measured roll exposure](roll-exposure.md) — sets the level the rule is reviewed at
  (the white stays measured at exposure 0, and the exposure moves where it renders)
- [The roll's slope from its span](span-roll-slope.md) — the span slope, built on
  the cap and floor, that this reviews on HDR
