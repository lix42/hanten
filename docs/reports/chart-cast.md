# Chart cast — the residual cast on ColorChecker frames

What colour the grey patches of a roll's chart frames come out, through Hanten's
settings and through other tools' conversions of the same frames. A grey patch with no
residual cast reads a\* = b\* = 0 (CIELAB D50, display white L\* 100); +b\* is yellow,
−b\* blue, +a\* magenta, −a\* green. **C\*** is how coloured the greys are overall.

Produced by the `chart-cast` skill (`nctool manifest charts cast render` / `measure`).
**Add one section per chart roll** and update the summary; numbers only, no images.

## Summary

As of 2026-10-15, one roll (`2026-10-12-Gold200`, overcast). Treat every line as one
roll's evidence until a second chart roll agrees.

- **Today's default leaves a strong blue residual cast**: b\* −15, flat from −2 to +3 EV.
  The midtone line causes most of it, the roll's white-balance gains the rest.
- **The roll's white balance never sees the chart.** It is fitted on the pictures, so it
  removes their average light (an illuminant cast) along with any roll cast; on a chart
  shot in other light it overshoots.
- **In density the residual is a near-constant blue offset** (−0.045 against green after
  the density scale), with slopes 3–4 % low. The bundled density scale fits this stock to
  within that.
- **The midtone line's two terms cancel on a\***: blue pushes magenta, red pushes green.
  Neither alone helps on this roll.
- **Best measured without a chart: `tilt` white balance with the midtone line off**
  (C\* 3.4–4.0), ahead of NLP, SilverFast and SilverFast CCR (5.5–6.9). It wins by
  applying less of an oversized gain in the shadows, not by fixing a slope.
- **Placement and highlight desaturation do not change the residual cast** (≤ 2 and
  ≤ 1.5 b\*).
- **The over end (+3/+4 EV) bends**: every tool drifts blue there, and red's density
  slope rises to 1.17 above 0.9 green density — the shoulder.

Follow-ups: `nf-calibration/offset-question`, `nf-calibration/stock-neutral-offset`,
`nf-calibration/chart-default-review`, `nf-scene-correction/midtone-neutral-fit`.

## 2026-10-12-Gold200

**Frames**: 2127–2133, a Passport Photo 2 bracket −2 … +4 EV (both targets: Classic 24,
Creative's 8 neutrals). Overcast, incident meter at the card; self-developed (CineStill
Cs41, an early roll of the kit); Plustek 8300i, SilverFast with automatic adjustments
locked. Conditions in full: the manifest's roll `conditions`.
**Build**: measured at `c40951c` (pipeline 17), reproduced exactly at `ce5a9e5` (render
fingerprint `f51d3397c7364160`). Roll measured on its 29 pictures; the chart frames'
tones taken from a measurement that included them. Outside exports: NLP, SilverFast and
SilverFast CCR, Adobe RGB, at the scan's size.
**Charts**: a private claude.ai Artifact, <https://claude.ai/artifact/JRAGP79niJ2JPmiuAp4Q8K>.

### Grey patches (L\* 15–95, all seven frames)

Hybrid placement, highlight desaturation 0.8 unless named.

| Configuration | C\* | a\* | b\* | dark b\* | mid b\* | light b\* | dark a\* | mid a\* | light a\* |
|---|---|---|---|---|---|---|---|---|---|
| default: gain, midtone line on | 15.4 | −0.5 | −15.4 | −15.0 | −18.9 | −13.3 | +0.3 | 0.0 | −0.9 |
| gain, line off | 7.8 | +2.0 | −7.2 | −8.9 | −7.3 | −6.7 | +3.0 | +3.8 | +0.7 |
| gain, red term only | 9.7 | −2.6 | −9.1 | −10.2 | −9.2 | −8.8 | −1.2 | −2.8 | −2.7 |
| gain, blue term only | 14.9 | +3.8 | −14.2 | −13.6 | −16.9 | −12.7 | +4.3 | +6.3 | +2.1 |
| tilt, line on | 15.1 | 0.0 | −15.0 | −4.3 | −20.3 | −14.0 | +0.1 | +0.7 | −0.5 |
| **tilt, line off** | **4.0** | +0.5 | −1.0 | +1.1 | +2.6 | −3.5 | +0.4 | +1.5 | −0.1 |
| tilt, red term only | 5.7 | −3.5 | −2.3 | +0.7 | +0.9 | −4.8 | −1.2 | −5.0 | −3.1 |
| roll balance off | 12.9 | −2.7 | +12.2 | +1.9 | +10.3 | +15.5 | +0.3 | −0.1 | −4.9 |
| `--rendering direct` | 8.4 | −2.5 | +7.9 | +5.4 | +7.7 | +12.2 | −0.5 | −3.1 | −4.4 |
| density offset `0,0,0.0447`, balance off | 2.9 | +1.2 | −1.3 | −5.5 | −1.6 | −0.2 | +2.3 | +2.9 | 0.0 |
| NLP | 5.9 | +0.5 | −4.7 | −1.2 | −3.0 | −5.7 | +4.9 | +1.8 | −0.5 |
| SilverFast | 6.9 | −1.4 | +0.5 | −2.2 | −2.3 | +3.9 | +5.3 | −2.7 | −2.4 |
| SilverFast CCR | 5.5 | −2.1 | −0.4 | −4.6 | −2.4 | +3.0 | +3.1 | −2.1 | −3.8 |

Bands: dark L\* 15–40, mid 40–70, light 70–95. Roll and frame placement land within
about 2 b\* of these rows. The density-offset row is in-sample (the offset was fitted to
these patches); what it shows is that one constant holds across the bracket.

### By bracket frame

Mean grey b\*, −2 … +4 EV:

| Configuration | −2 | −1 | 0 | +1 | +2 | +3 | +4 |
|---|---|---|---|---|---|---|---|
| default | −16 | −16 | −16 | −17 | −16 | −17 | −9 |
| tilt, line off | +2 | +1 | +1 | −1 | −1 | −5 | −6 |
| density offset, balance off | −4 | −3 | −2 | 0 | +1 | −1 | 0 |
| roll balance off | +8 | +10 | +12 | +14 | +15 | +14 | +14 |
| NLP | 0 | −2 | −2 | −3 | −5 | −9 | −11 |
| SilverFast | +6 | +4 | +2 | +3 | +1 | −6 | −8 |
| SilverFast CCR | +5 | +2 | 0 | 0 | 0 | −4 | −5 |

### Density (`charts.json`, 98 grey patches, after scale 1, 0.84, 0.73)

| green density above base | 0.2 | 0.4 | 0.6 | 0.8 |
|---|---|---|---|---|
| red − green | +0.017 | +0.001 | −0.008 | −0.006 |
| blue − green | −0.037 | −0.046 | −0.043 | −0.051 |

Fitted against green: red slope 0.973, blue 0.963. Mean over 0.2–1.0 (84 patches): red
−0.0003, blue −0.0447. Above 0.9 (the +3/+4 frames, 11 patches) red's slope is 1.17.

Blue's residual shrinks a little toward the film base. A **density offset** would stay
constant down to the base; a shift in **exposure** (the light, or one layer's speed)
fades in the toe. This roll cannot tell the two apart, which matters for where a
per-stock value lives (`nf-calibration/offset-question`).

### Colour patches

On the frame whose mid-grey (D4) renders nearest the card's L\* 51, mean hue error of the
saturated Classic patches: tilt with the line off 9.5–10.3°, SilverFast CCR 10.1°,
SilverFast 11.7°, gain with the line off 13–14°, NLP 17.1°, `direct` 18.2°, a midtone line
with the blue term 15–22°. Hybrid placement and NLP never bring D4 below L\* 66, so
their colour numbers stay confounded with lightness.

### Limits

One roll, one light, one developer. Overcast is bluer than the film's balance, so a few
b\* of blue can be the true light — an illuminant cast. The outside tools may balance
each frame on their own, which a chart frame flatters. No 180°-rotated frame was shot,
so uneven light across the card is assumed, not checked.
