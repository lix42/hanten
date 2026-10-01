# Exposure buckets within a roll

**Closed — not needed (2026-10-01).** The one known case has no exposure change the
roll's rule can see, so a group would measure what the roll already measures.

## Outcome

- **No split on 09-28 moves the exposure.** Cutting the roll's shipped `level_stops`
  (`measure-roll`, `../temp/roll-exposure/reports-impl/`) at every frame and taking each
  side's median: the halfway split (1980–1996 / 1998–2014) puts the groups 0.05 EV apart,
  the largest of 27 cuts 0.24 EV (at 2011, a last group of four). Every group lands at the
  roll's +1.39 within the 0.3 EV per-frame split `roll-exposure`'s round 2 found.
- **Detection has nothing to find.** Scene content moves a frame's level by up to 2 stops
  on this roll; an automatic change point would find runs of night scenes, the per-frame
  failure the median rule exists to avoid.
- **What is left is per frame**, and `frame-level-trim`'s. A roll with a deliberate,
  known exposure change would reopen the question as a new task.

## Goal

Detect groups of frames within one roll that were exposed differently, and measure one
exposure per group instead of one for the roll: a meter that drifted, a battery changed
mid-roll, a camera swapped. Each group then renders at a normal level, and frames within
a group stay consistent with each other.

## Design

What is known:

- **The motivating case**: `nc-assets` roll `2026-09-28-Portra400-dark`. The user reports
  a weak camera battery for the first half and a change mid-roll, so the meter may have
  read differently before and after.
- **The levels did not show it.** Per-frame log-average luma in frame order
  (`../temp/roll-exposure/reports/`) has no step: the medians run −2.1 (1980–1989), −1.7
  (1990–1999) and −1.97 (2000–2014). Scene content varies more than the suspected
  exposure shift. So detection is the hard part, not the per-group exposure.
- **A group is a roll in miniature**: `roll-exposure`'s median-level rule applies to it
  unchanged. The failure to avoid is the per-frame one, where a night scene reads as
  under-exposure.

Open:

- **Detection**: automatic (a change point in frame order, clustering by level) or stated
  by the user (a frame range). With no visible step on the one known case, a stated range
  may be the only reliable input.
- **How a group is stated in the recipe**: per-frame entries in `roll.frames`, or a
  grouping key.
- **What stays roll-wide**: the film base, white balance and white are one per roll today.
  Whether a group may differ in any of them, or in exposure only.
- **How it relates to `frame-level-trim`**: the trim adjusts around whichever exposure
  the frame's group has.

## How to Verify

- On a roll with a known exposure change (09-28-dark with the user's split, or a roll shot
  deliberately with a change), each group lands at a normal level and the report names the
  groups.
- A roll with no change gets one group, and renders byte-identically to `roll-exposure`.

## Dependencies

- [A measured roll exposure](roll-exposure.md) — the per-roll rule a group reuses, and the
  per-frame level detection would read
