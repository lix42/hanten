# The hybrid's span target with a toe

## Goal

Re-decide the hybrid placement's span target R (`roll.hybrid_span_stops`, 7;
`--hybrid-span`) once a toe holds the dark end. Raising it is a steeper default.

## Design

Known:

- R was reviewed twice without a toe (the poor-development spike's a4,
  `envelope-hybrid-placement`), and no code reviews it today.
- The hybrid has less contrast than a per-frame slope by design: its darkest 1 % sits at
  L\* 10.6–14.3 (per-roll medians), `frame`'s at 8.2, CCR's at 4.7–7.4
  (`hybrid-slope-bounds`).
- A steeper slope under the hybrid (`--contrast` 1.15, 1.29) lost to ×1 without a toe
  (`docs/spike/midtone-contrast-probe.md`).

Open:

- Whether a toe changes that: whether R can rise, and how far, without losing the
  restraint the hybrid was chosen for.

## How to Verify

- A review under the default placement with the toe, today's R against one or two higher
  values, the darkest 1 % and shadow noise measured beside each arm.

## Dependencies

- [More midtone contrast, the ends held by a toe and the
  shoulder](../nf-display-stages/midtone-contrast-toe.md) — the toe that may let R rise
