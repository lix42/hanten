# An automatic turn-off per midtone-line term

## Goal

Turn the midtone line's red or blue term off on its own when a signal in the roll says the
roll's content misled that term, the automatic counterpart of `--midtone-neutral red|blue`
(`midtone-line-terms`). Neither term is special: a roll's dominant colour can fool either,
or both.

## Design

Known (`midtone-line-terms`, 2026-10-13; `../temp/midtone-line-terms/`):

- **Which term goes wrong.** Patch casts with each term off, on the archive rolls with a
  line and Lomography 100: the blue term was misled on 09-14 and 09-18 (sea) and
  Lomography 100 (warm light), the red term only on 09-09, and slightly. When blue was
  misled, red added almost nothing.
- **The terms have been needed.** Development casts take both: 09-29 (67 with both, 113 or
  288 with one) and 09-20. Turning a term off automatically can cost a roll that needs it.
- **No in-roll signal yet.** `correction-confidence` found none that separates a roll of
  one dominant colour from a real cast for the whole correction. Lomography 100 suggests one
  for a single term: its vote disagreeing with the roll's near-neutral pixels at the same
  brightness. Untested, and one roll.

Open:

- The signal, tested on the calibration rolls (ground truth) and the archive's labelled
  cases.
- A roll that fools both terms, or reads as a green cast, which no single term can drop.
- Whether the report names a doubtful term, the way `confidence.advice` names the
  correction, rather than turning it off.

## How to Verify

- An automatic rule turns a term off on Lomography 100 and leaves the poor-development
  rolls' terms on.
- Patch casts on every roll with marked neutrals are no worse than with both terms on.

## Dependencies

- [Each term of the midtone line on its own](midtone-line-terms.md) — the manual switch
  and the per-term study
- [Capture the calibration frames](../analysis/calibration-frame-capture.md) — the second
  labelled case
