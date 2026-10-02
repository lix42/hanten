# A bounded lift for a thin frame

## Goal

A bounded fine-tune for a frame far thinner than its roll, **on by default with an
off switch** (decided 2026-10-01: it is taste, not quality, and viewers tend to prefer
bright). The roll's exposure alone leaves such a frame dark. The frame gets a steeper slope and more exposure that raise its white a bounded amount with the
film base held where the roll renders it. It is **not** solved to put the white at diffuse
white: a thin frame need not hold a white at all (user, 2026-10-01). More grain is the
price, within a bound, and the recipe and report show what was applied.

It also owns the question `nf-look/scene-range-mapping` asked, which was folded in
2026-09-29: whether a frame's own measured range should set its render as a per-frame
opt-in. Answer: no, beyond this bounded lift, and a **flat** frame (one surface filling
it) gets no lift at all, the small default one included.

## Design

Settled (2026-10-01; evidence in the progress log):

- **The lift**: the frame's white rises `Δ` = 1 stop (chosen over 0.5 by review) with the
  base held: slope `k0 + Δ/(white − base)`, exposure solved to keep `k·(base + e)`; the
  slope is bounded at 2.4, and a frame the bound holds is reported by name. It replaces
  `frame-level-trim`'s small lift in the entry. Delta, not total.
- **Which frames**: rendered white at or under +0.9 stop and at least 30% of the luma within
  a stop of the base. No statistic separates an underexposed frame from a night scene;
  review preferred the lift on night frames too.
- **Flat guard**: p5–p95 luma spread under 1 stop gets no lift, for both lifts. Two such
  frames in ten rolls (1612, 1719), both worse lifted.
- **Keys**: written by `measure-roll` unless `--no-thin-lift` (or `--no-frame-lift`); a
  `roll.frames` entry's `slope` (a slope, not a
  white: it is chosen), moved to `roll.frame_slope` / `--roll-frame-slope`;
  `--frame-lift off` turns off both halves.
- **Scope**: thin frames only; dense or flat frames get no per-frame range mapping.

Open:

- **The confirmation round** on a roll not used to choose the values. 09-11 and 09-13
  carried night frames into round 1, so they are not independent.
- **Noise was not measured**; grain was judged by eye only. Slope 2.4 passed review on
  two frames (2005, 1983); written slopes on the archive reach 2.11.

## How to Verify

- The lift beats or matches the roll's render on the frames it fires on and changes no
  other frame's entry; a flat frame loses its small lift.
- Under `--no-thin-lift`, `measure-roll --out` writes the same values as before this
  task, except the new `null` keys and the flat frames' dropped lifts.
- A frame that the bound stops from being fully lifted is reported by name.
- A review round on at least one more roll with thin frames confirms the bounds.

## Dependencies

- [A measured roll exposure](roll-exposure.md) — the lift is measured on top of the roll's
  exposure, and "thinner than the roll" is relative to it
- [`measure-roll` places the roll's white](roll-white-rule.md) — the per-frame whites and
  the clamp entries it extends
