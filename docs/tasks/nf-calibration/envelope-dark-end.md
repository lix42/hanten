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
- Several frames share the dark end there, so whether this is one outlier frame or the
  envelope's rule itself is not yet known.

## Open

- What sets the dark end that pushes the roll: one frame's dark end, its level, or the
  hybrid's exposure rule (α of a frame's offset from the roll's median level).
- Whether the fix is a robust dark end (a percentile over frames instead of the minimum),
  a per-frame exemption, or a change to how the exposure is held.
- Whether other archive rolls show it — 09-28 Portra400-dark is the obvious check.

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
