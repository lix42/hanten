# A home for the film-stock data

## Goal

Decide what happens to the digitized film-stock data when `characteristic` leaves
reconstruction, so that live code, a registry and a folder of datasheets do not
quietly become orphans.

## Design

- **What exists.** Ten digitized stocks with their per-channel curves and a
  registry (`src/algo/film_stock/`), reached by `--film-stock`, plus the sheets
  they were read from in `docs/datasheets/`. Their only runtime consumer is the
  `characteristic` reconstruction curve, which the fixed decode replaces.
- **The data is also evidence.** Part 1 quotes the registry's per-stock spread
  (`d` 0.542–0.699, red film gamma 0.53–0.61) in the argument *for* fixed nominal
  values — the case against per-stock decoding rests on the very tables that lose
  their consumer. Deleting them would delete the provenance of the decode's
  constants.
- **The natural future consumer is this stage.** Per-stock normalization is a
  later addition to the look (design-update Part 2): normalizing a stock toward a
  common aim is a creative choice applied after a stock-agnostic decode, not a
  second decode. That is a reason to keep the data, not to build the feature now.
- **Decide per piece**, and say why: the curve tables, the registry and its
  lookup, `--film-stock` as provenance, and the inversion code that reads a curve
  backwards. They do not all have the same answer — the inversion is the part tied
  to the retiring path.
- **Anything kept needs a named consumer or a written waiver.** CLAUDE.md forbids
  new dead API without a comment saying who will use it; a table kept as evidence
  for a documented constant is a legitimate answer, an unreferenced module is not.

## Open questions

- Does `--film-stock` keep a meaning at all before per-stock normalization ships —
  recorded provenance, or removed and re-introduced later?
- Do the datasheet PDFs stay in the repo, or move to the assets folder with the
  other measurement inputs?

## How to Verify

- After the retirement lands, nothing in `src/` or `scripts/` references a removed
  stock path, and no doc cites a datasheet that moved without a redirect.
- Whatever is kept either compiles with a consumer or carries the waiver comment.
- The decode's nominal constants still cite a source that a reader can open.

## Dependencies

- [The look stage](stage.md)
