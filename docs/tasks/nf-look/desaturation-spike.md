# Spike: what form should highlight desaturation take?

## Goal

Settle the form of the operator [path to white](path-to-white.md) would ship, and which
mechanism it has to reproduce. The design says chroma goes to zero as a pixel approaches
diffuse white; it does not say *measured on what*, and the two plausible answers were
expected to produce visibly different pictures. They do not — see the report.

## Design

`docs/spike/highlight-desaturation.md` is the result. The parts that were structural and
settled before any render are in `docs/reports/three-way-gold200.md`: that per-channel
compression against a common ceiling is what converges channels, that no nc display tone
can do it, and that the operator belongs pre-branch referenced to diffuse white.

What this spike had to find was the **form** — a per-channel curve, as film and paper
and all three outside converters use, or a chroma pull toward the neutral axis. Both were
reachable without the migration: the sigmoid's shoulder already is the first, and the
second was a throwaway patch in `render_split::display_source`, reverted afterwards.

Held fixed throughout: one anchor (white pinned to each frame's own p97, so both
families can act), one rendering otherwise, and brightness matched before any colour was
judged.

## Open questions

All four were answered or superseded; the report carries them. In short: what
"approaches white" is measured on must include **distance from the neutral axis**, not
luminance alone — that is the spike's main result. The cost on saturated colour is what
the guard exists to remove. How much cast it hides was not measured and remains open for
`path-to-white`. The gamut map's share is still unseparated: nothing reachable by flag
turns it off, and `--display-tone shoulder` failed as a separator because it flattens
rather than shapes.

## How to Verify

**Done.** The result is [`docs/spike/highlight-desaturation.md`](../../spike/highlight-desaturation.md),
with the execution trail in `docs/progress/nf-look.md`. The verdict is folded into
[path to white](path-to-white.md)'s Design, which is the task that ships the operator.

## Dependencies

None — the per-channel half runs against today's binary and the hue-preserving half is a
throwaway patch, so it can run before [the look stage](stage.md) exists.
