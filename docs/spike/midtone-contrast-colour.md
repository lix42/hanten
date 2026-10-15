# Colour under a steeper midtone

Should saturation follow the opt-in `--contrast 1.3`? Produced by
`nf-look/midtone-contrast-colour`, 2026-10-15.

Since `nf-look/contrast-on-luminance`, contrast moves luminance only, and colour has its
own slope: the roll's × 1.15 × `--saturation`. So `--contrast 1.3` keeps every pixel's
channel ratios.

Measured on 11 frames from `2026-09-18-Gold200`, `2026-09-20-Portra400` and
`2026-09-29-Ektar100` (the saturation review's), each roll re-measured whole with the
build that rendered them (`pipeline_version` 17). Default rendering, hybrid placement,
dark target 5.5, highlight desaturation on, SDR Display P3. Scripts, renders and raw
figures are in `../temp/midtone-contrast-colour/`.

## What was compared

×1 (the default), and ×1.3 with `--saturation` 1 (colour held), 1.15 (colour follows about
half) and 1.3 (colour follows fully).

## Measured

CIELAB C\*, a 5 % inset:

| | ×1 | ×1.3 held | ×1.3, 1.15 | ×1.3, 1.3 |
|---|---|---|---|---|
| frame mean | 14.2 | 14.1 | 16.0 | 17.8 |
| 14 marked colour patches | 18.5 | 16.2 | 18.1 | 20.0 |
| 17 marked whites (leftover cast) | 7.1 | 6.3 | 7.0 | 7.7 |

- **With colour held, chroma does not drop measurably at any lightness.** Held ÷ ×1 per
  L\* band, median over frames: 1.03 under L\* 25, 0.99 at 25–50, 1.02 at 50–75, 1.07
  above. Kept channel ratios keep C\* roughly in proportion to L\*, so any
  undersaturation was a judgement by eye, not a measured fall.
- **The held patches' drop is bright colour pushed toward white.** The 1820 sand beach
  moves from L\* 96 to 99 and loses chroma (C\* 15.4 → 4.0), as do 1883's sunset clouds
  (15–16 → 6). No saturation arm brings it back, and highlight desaturation is not the
  cause (C\* 4.0 with it off). The SDR top leaves no room for chroma there.
- **Saturation raises the whites' leftover cast and the share outside sRGB** (1904: 3 %
  held, 7 % at 1.15, 13 % at 1.3).

## Verdict (user review)

- **`--saturation 1.15` looks best beside `--contrast 1.3`.** It is the suggested pairing,
  documented in `docs/using-nc.md` (the look).
- **A documented pairing, not a coupling.** `--contrast` still leaves colour alone, so no
  default render, recipe or `pipeline_version` moved. Coupling them in code would have
  changed what `look.contrast` means and broken that promise.

## Not settled here

- **Bright colour under ×1.3.** Bright coloured surfaces still lose chroma at the SDR top.
  That depends on where ×1.3 places the highlights, not on saturation.
