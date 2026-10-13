# Each term of the midtone line on its own

## Goal

Let the midtone line's red and blue terms be turned off one at a time, by hand, beside
today's whole-line `--midtone-neutral`. Neither term is special: a roll's dominant colour
can fool either, or both. The automatic turn-off, on a signal, is
[`midtone-term-signal`](midtone-term-signal.md).

## Design

Known (`lomo100-cast-vs-ccr`, 2026-10-12; `../temp/lomo100-cast/`):

- **What fooled the line on Lomography 100.** On this warm roll the blue term voted the
  midtones short of blue and added that much back, though the white patches were already
  slightly blue: 0.11–0.18 stop under today's gains, 0.13–0.27 under the two-point tilt.
  The same warmth also biased the gains in both red and blue.
- **What dropping the blue term did.** Patch distance from neutral, lit cloth+wall / cloud:
  TL 6.8 / 15.2 → TR 5.1 / 12.7. The user found TR acceptable on every frame but 2061.
  The red term kept in TR helped lit cloth+wall but cost cloud (T, with no line, scores 11.4).
- **TR's blue term was dropped by hand, from this roll's patches.** A real conversion has
  no patches, so TR's numbers are not what a rule would get automatically.
- **The whole line already has a switch** (`--midtone-neutral` / `roll.midtone_neutral`),
  because a roll of one dominant colour can mislead it (`midtone-neutral`,
  `correction-confidence`).
- **No in-roll signal yet.** `correction-confidence` found none that separates a roll of
  one dominant colour from a real cast, at the level of the whole correction.
- **The terms have been needed.** 09-29's cast was a per-channel slope error plus an offset
  on blue, and the whole line fixed it (`docs/spike/poor-development.md`). So turning a
  term off automatically can cost a roll that needs it.

Open:

- ~~The manual form~~ — one selector, `--midtone-neutral on|red|blue|off` (2026-10-13).
- ~~The whites when one term is off~~ — they stay measured with the whole line; the term
  is a render-time choice only, since a measurement never states a switch (2026-10-13).
- A roll that fools both terms, or shows as a green cast, which no single term can drop.
- The automatic signal per term, and whether the report names a doubtful term, moved to
  `midtone-term-signal` (2026-10-13), which waits for the calibration rolls.

## How to Verify

- Patch casts with each term off, across every archive roll with marked neutrals.
- `docs/using-nc.md` describes the switch.

## Dependencies

- [Hanten against SilverFast CCR on the Lomography 100 roll](lomo100-cast-vs-ccr.md) — the
  case and its arms
- [A midtone neutral measured per roll](midtone-neutral.md) — the line and its switch
