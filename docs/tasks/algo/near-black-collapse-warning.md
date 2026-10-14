# Near-black collapse warning

> **Done 2026-10-13.** `pipeline::near_black` measures; `cli::near_black_warning` warns.
> The open questions are answered below; the evidence is in `docs/progress/algo.md`.

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
  display-referred buffer before the encode. **Each channel's p99.9 on the
  display-referred buffer before the encode** (`pipeline::near_black`), where diffuse
  white is 1 for every destination. A percentile, not the maximum, so a glint cannot hide
  a collapse. Per channel, like part 2.
- The threshold, set so every real frame in `../nc-assets`, the darkest included,
  stays silent. **10 stops below diffuse white.** The darkest of 246 real frames sits
  0.83 stops below under its roll's recipe, and 5.68 stops below with only the film base
  stated (the thin roll, uncorrected). That leaves a 4.3-stop margin.
- Whether it should replace part 2's exact rule or sit beside it. **Beside it.** Part 2
  needs no threshold and covers the film master, which has no display white. A channel
  part 2 names is not named again.

> **2026-10-13:** on today's chain `--exposure=-20` quantizes to all zero in SDR and
> part 2 catches it. The cases part 2 misses are now `--exposure=-10` and `-14` on SDR,
> PQ, linear float and gain map, and `-100` on linear float.

## How to Verify

- The `--exposure=-20` case above warns; part 2's cases still warn.
- Every real frame converts at defaults with no collapse warning (an `#[ignore]` probe
  printing derived numbers only).

## Dependencies

- [Density safety bounds](density-safety-bounds.md)
