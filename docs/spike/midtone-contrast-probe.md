# More midtone contrast, before a toe

Does a steeper slope than the placements set today render better, with nothing new holding
either end? Produced by `nf-display-stages/midtone-contrast-probe`, 2026-10-11; the toe it
asks for is `nf-display-stages/midtone-contrast-toe`.

Measured on two whole rolls from the full archive, `2026-09-18-Gold200` (35 frames) and
`2026-09-29-Ektar100` (31), re-measured with the build that rendered them (80ef96c, dirty
with this task's doc edits only; `pipeline_version` 14). Default rendering and placement
(the hybrid), SDR sRGB JPEG, display black off. Scripts, renders and raw figures in `../temp/midtone-contrast-probe/`.

## What was compared

`look.contrast` at 1, 1.15 and 1.29 (≈ a 7-stop span spread over 9, the "span plus 2 EV"
idea). It multiplies the slope the placement chose, after the maximum and the envelope, and
pivots at mid-grey; exposure, the placement and the colour's slope were identical across
arms. Fit range is reinhard in every arm, so the white has its shoulder; the dark end has
nothing.

## Measured

Roll medians, 09-18 / 09-29, rendered L\* over a 5 % inset:

| | ×1 | ×1.15 | ×1.29 |
|---|---|---|---|
| p1 | 12.3 / 11.5 | 8.7 / 7.9 | 6.0 / 5.4 |
| p50 | 51.1 / 53.9 | 51.3 / 54.5 | 51.5 / 55.1 |
| p99 | 85.7 / 83.0 | 89.1 / 86.5 | 91.6 / 89.2 |
| p75 − p25, stops | 1.89 / 2.04 | 2.17 / 2.36 | 2.44 / 2.53 |
| pixels under L\* 5 | 0 % / 0 % | 0 % / 0 % | 0.5 % / 0.7 % (max 4.0 %) |
| a channel at 255 | 0.2 % / 0 % | 0.6 % / 0 % | 1.2 % / 0.1 % (max 14 %) |

- **The dark end takes most of the change.** Each end moves by its distance from mid-grey,
  and the dark end sits further away: an estimated median 58 % (31–63 %) of the extra
  stops, 1.18 look stops at ×1.29 (from slope and exposure, without the inset or fit
  range). Measured, frames whose darkest 1 % lands under L\* 5: 0 / 0 / 23 of 66 (×1 /
  ×1.15 / ×1.29); under L\* 10: 8 / 45 / 51.
- **The white holds.** Lightness at or above 98 stays a median 0 % (max 10.5 %, 2037); the
  channel clipping is saturated colour (1800 at 14 %), not white.

## Verdict (user review)

- **The default stays at ×1**, preferred over both.
- **×1.29 is a reasonable high-contrast choice**, offered as an opt-in: `--contrast 1.3`,
  documented in `docs/using-nc.md` (the look). No preset or switch: presets were retired
  (`nf-look/look-presets`), and the knob already exists.
- **×1.15 is too close to ×1** to be worth a setting.

## Not settled here

- **A toe.** ×1.29 was judged with its shadows sinking; held up by a toe it is a different
  render and might compare differently with ×1.
- **Other placements.** Only the hybrid was measured. Under `--placement roll` a low- or
  high-key frame is not centred first, so the multiplier pushes it further to its own end;
  `frame` pins each white at L\* 93 before the multiplier, which moves it off.
  Checked after the toe exists.
- **Colour.** It stayed at the roll's slope in every arm
  (`nf-look/midtone-contrast-colour`; answered in
  [`midtone-contrast-colour.md`](midtone-contrast-colour.md): pair ×1.3 with
  `--saturation 1.15`).
- **Noise.** A steeper slope raises grain in stops; not measured here.
