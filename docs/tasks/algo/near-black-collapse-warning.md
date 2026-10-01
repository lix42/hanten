# Near-black collapse warning

## Goal

Warn when a render collapses to near-black without any channel reaching exactly 0:
part 3 of [`algo/density-safety-bounds`](density-safety-bounds.md), split out when
parts 1 and 2 landed (2026-10-01) because it alone needs real scans.

## Known

- Part 2's warning fires only when a channel has **no written sample above 0**. It
  misses a render that is black in practice:
  - `--exposure=-20` on `tests/fixtures/hdr-48bit.tif` (roll stated) writes 31–59 % zero
    samples per channel with a maximum code of 3, and no warning;
  - an HDR float TIFF keeps tiny positive floats (`--exposure=-100` writes a mean of
    ~1e-30), and PQ never encodes 0 (PQ of 0 is ~7e-7), so neither can reach part 2;
  - the existing "HDR output carries an SDR-range signal" warning already fires on the
    HDR cases measured, for another reason.
- The per-channel maximum part 2 added (`OutputStats::max`, internal) is one input a
  rule could use; the mean is in the report already.
- The hard part is the false-positive guard: a legitimately low-key frame must stay
  silent. It is a warning, never an error (`--strict` promotes).

## Open questions

- The measure: dynamic range, a near-black fraction, the maximum against the
  destination's reference white, or a mix — and per destination or on the
  display-referred buffer before the encode.
- The threshold, set so every real frame in `../nc-assets`, the darkest included,
  stays silent.
- Whether it should replace part 2's exact rule or sit beside it.

## How to Verify

- The `--exposure=-20` case above warns; part 2's cases still warn.
- Every real frame converts at defaults with no collapse warning (an `#[ignore]` probe
  printing derived numbers only).

## Dependencies

- [Density safety bounds](density-safety-bounds.md)
