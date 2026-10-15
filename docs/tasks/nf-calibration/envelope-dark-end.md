# One dark frame brightens the whole roll

## Goal

Keep one genuinely dark frame from pushing every other frame on its roll up. The hybrid
gives each frame its own contrast inside the roll's envelope, so the frames keep their
relative positions; the envelope's dark end is the darkest point any frame reaches. A
roll with one real dark frame puts that end very low, and the rest of the roll renders
brighter for it.

## Known

- Found by review (user, 2026-10-13) on `2026-10-12-Gold200` (29 pictures,
  `../temp/gold200-1012-roll-review/`, v16 `266d696`): under the default hybrid the roll
  looked pushed up; the set's Hanten arm was re-rendered `--placement frame` instead.
- The measurement (`converted/nc/2026-10-13/2026-10-12-Gold200/measure-roll-29pictures.json`
  in the archive): envelope −3.82…+0.88 stops; frames' dark ends at −3.80…−3.82 on 2107,
  2114, 2117 and 2098; the lowest levels are 2107 and 2114 (−3.0). Median frame exposure
  +0.26 EV under the hybrid against −0.52 EV under `frame`.
- 2026-10-14: **not one frame.** 21 of the 29 frames have their dark end within 0.1 stop
  of the film base, so a percentile over frames changes nothing. The cause is the envelope's
  floor on a thin roll: the roll's exposure lifts the base with the level, per-roll renders
  the darkest frame only −2.29 look stops under mid-grey, and the hybrid raises each frame's
  exposure to keep its dark end above that. On this roll the envelope is 8 look stops wide,
  so every frame fills it. The lift grows with the roll's exposure (09-11, 09-13, 09-28,
  10-10 LomoColor, 10-12 Gold200).

## Outcome (2026-10-14): no change

Reviewed (`../temp/envelope-dark-end/`) at v17, with the 5.5 dark target: a floor no paler
than `frame`'s dark end was too dark; flattening instead of lifting was liked on the thin
rolls but moves 130 of 246 archive frames, reviewed rolls included. Today's hybrid stays.
`frame` at a dark target of 5 differs little from 5.5.

## How to Verify

- On `2026-10-12-Gold200`, the hybrid no longer renders the roll brighter than its
  frames warrant, by review beside `frame`.
- The archive's other rolls do not move where they were judged right; a drift-gate row
  if the default render changes.

## Dependencies

- [Envelope hybrid placement](envelope-hybrid-placement.md) — the envelope and the rule
  under question
- [The hybrid's span target](hybrid-span-target.md) — R 8, which on a thin roll already
  brightens via the envelope
