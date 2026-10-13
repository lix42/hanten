# The hybrid's span target

## Goal

Re-decide the hybrid placement's span target R (`roll.hybrid_span_stops`,
`--hybrid-span`) now that the dark floor exists. Raising it is a steeper default.

## Design

Known:

- R was reviewed twice before the floor (the poor-development spike's a4,
  `envelope-hybrid-placement`).
- The hybrid has less contrast than a per-frame slope by design: its darkest 1 % sits at
  L\* 10.6–14.3 (per-roll medians), `frame`'s at 8.2, CCR's at 4.7–7.4
  (`hybrid-slope-bounds`).
- R is not `--contrast`: it acts inside the placement, so the envelope and the 3.5
  maximum still bound the slope. A steeper `--contrast` under the hybrid lost to ×1
  (`docs/spike/midtone-contrast-probe.md`); that does not settle R.

Settled (2026-10-13, review and the user):

- **R = 8** (`pipeline_version` 16), over 7 and 9, on 09-20 and 09-28.
- The envelope holds the dark end, not the floor: the floor barely acts at any R. On a
  thin roll a higher R brightens the frames rather than deepening them.
- α (`hybrid_level_keep`) stays 0.6: R 8 was judged with it.

## How to Verify

- A review under the default placement with the floor on: R 7 against 8 and 9, the
  darkest 1 % and shadow noise measured beside each arm.

## Dependencies

- [More midtone contrast, the ends held by a toe and the
  shoulder](../nf-display-stages/midtone-contrast-toe.md) — the dark floor, judged with it on
