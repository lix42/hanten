# Two renderings: `direct` and `default`

## Goal

A `--rendering direct|default` selector (recipe `rendering`, new flow only) that chooses
the base every stage knob starts from. `direct` loses as little information as possible
and applies only what the container needs: the handoff to Lightroom or Photoshop, and
the rendering the calibration loop holds fixed. `default` is what our code produces
from measured values alone: the roll section applied, today's defaults for the rest.

## Design

The design is `docs/design-update.md`, Part 2, "Two renderings: `direct` and `default`"
(2026-09-27). What this task builds on:

- **Re-planned 2026-09-27.** This task began as one Adobe RGB destination with an
  identity scene correction and an empty look. Two things moved under it. The roll's
  measurements moved into rendering (`nf-calibration/roll-white-rule`), so "minimal" now
  has two readings, and each gets a rendering. And the gamut became a knob
  (`nf-destinations/preset-set`), so this is a rendering choice, not a destination.
- **The values, per rendering**, are the design-update table. `direct`'s are its own
  pinned constants (contrast `2.0 / 1.8`, display black 6 stops, reinhard at 6 stops,
  desaturation off, white balance identity), never today's defaults read through.
- **Explicit knobs build on the base.** `--white-balance` multiplies the base gains;
  every other knob, `--contrast` included, replaces its base value
  (`nf-look/contrast-definition` changes that for contrast).
- **`default` without a roll section** renders with the fallbacks and warns, naming
  `hanten measure-roll`. The fallback values are `nf-calibration/no-roll-defaults`'s.
- **`direct` defaults the destination to HDR** (user, 2026-09-27): range `hdr`,
  transfer `linear`, container `tiff` — the 32-bit float BT.2020 TIFF — and gamut Adobe
  RGB, which the table reaches only when SDR is stated. `default` keeps SDR Display P3.
  So the destination's unset-axis defaults depend on the rendering (`destination::resolve`
  takes them from it), and a stated axis still wins. The global default stays
  `nf-destinations/default-destination`'s.
- **The gamut is ready** ([`output/adobe-rgb-gamut`](../output/adobe-rgb-gamut.md)), and
  `nctool metrics` already maps `("adobe-rgb", "native")` to `adobe-rgb`.
- **Keeping `direct` current**: one struct built without `..`, so a new stage knob fails
  to compile until its `direct` value is decided; a pinned test of the resolved values
  whose failure message names the module doc holding the procedure (classify by the
  principle; log a move in `nf-calibration`'s progress).

Open:

- **Set versus default.** Once the base depends on the rendering, a knob the user did
  not state must not be written into the sidecar as if they had, or a `default` sidecar
  replayed under `direct` carries desaturation 0.8 as a stated value. The knobs a
  rendering sets become "unset means the rendering's base", and the report records the
  resolved values.
- **`RunProfile`**: `direct`'s default rides `NewFlowF32Tiff`, which `preset-set` left
  provisional (counted, not measured), so it is measured here on two frame sizes, as
  `nf-destinations/memory-profiles` requires; and whether the Adobe RGB SDR form shares
  `NewFlowU16Tiff` with Display P3 (same buffers and shape, so it should).
- How the report states the rendering, whether the roll section was applied, and where
  each resolved value came from (rendering base, roll, fallback, or stated).

## How to Verify

- `--rendering direct` with no other knob: the roll section is reported as not applied,
  white balance and the grade are identity, desaturation is off, and the output is the
  float BT.2020 TIFF (`nctool metrics` reads it as `linear-bt2020`); with `--range sdr`
  it reads as Adobe RGB under `nctool metrics --space adobe-rgb`.
- On a correctly exposed frame, mid-grey lands mid under `direct` across stocks, with no
  per-frame correction.
- Two decode candidates rendered through `direct` differ only in the decode.
- A sidecar replays exactly under either rendering, and a stated knob wins over either
  base.
- The pinned test of `direct`'s resolved values, and the memory measurement recorded.
- `docs/using-nc.md` updated by running the binary; design-spec §9 states the key.

## Dependencies

- [The destination set](preset-set.md)
- [Adobe RGB (1998) as an output gamut](../output/adobe-rgb-gamut.md)
- [The roll's measurements as their own recipe section](../nf-calibration/roll-section.md)
  — `default` applies the section and `direct` does not
