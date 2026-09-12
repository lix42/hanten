# Metrics Chart Design

## Goal

Settle **what the charts are** — the encoding each measurement gets, how many
visuals they collapse into, what renders them, and how they decompose into
components. App-independent on purpose: the answers here are about reading
numbers, not about `review.json` or the review app's layout.

Split out of [Metrics visualization](metrics-visualization.md) on 2026-09-10,
which keeps the integration half. This is the harder one.

## What exists

`nctool metrics {image,roll,table}` (`analysis/conversion-metrics`) is the data
source; this task adds no measurements. At `schema_version` 2 one record per
(image, config) carries:

| Group | Shape |
| --- | --- |
| `tone.histogram` | 4 series (luminance, R, G, B) × 200 counts, **one per L\* unit**, running to twice diffuse white so an HDR render's headroom is drawable and an SDR render's shortfall below white is visible; names `mid_grey_bin` and `diffuse_white_bin` |
| `tone.percentiles_stops` | 11 points, `p0.1`…`p99.9`, log2 stops re mid-grey |
| `tone.bands` | 7 named bands, cut on equal L\* steps, fractions summing to 1 |
| `tone.*` scalars | `key_stops`, `contrast.*`, `toe_span_stops`, `shoulder_span_stops` |
| `color.cast_by_tone_band` | per band: `fraction`, `pixels`, `sparse`, `mean_a`, `mean_b` |
| `color.hue_sectors` | 6 sectors: `fraction`, `mean_chroma`, `mean_hue` |
| `color.*` scalars | `balance_stops` (incl. `b_over_g`), `mean_cast`, chroma percentiles |
| `endpoints` | per-channel occupancy at/above white and at/below black |

Three properties of that shape decide encodings, all new on 2026-09-10:

- **The histogram is the first drawable field in any of these records**, and it
  was added for this task. Its **luminance** series is the primary single-frame
  view — see the mode split below.
- **Bands and histogram share one axis.** Both are cut in L\*, so every band edge
  falls exactly on a bin edge: a band overlay draws on the histogram without
  interpolating, and the two cannot disagree.
- **`sparse` is a field now, not something a chart must derive.** A band under
  0.1% of the region is flagged, and the roll rollup withholds `crossover_*` when
  a contributing band is sparse. Two names that are easy to conflate: the
  histogram's `above_range` counter (past twice diffuse white) is not the band
  `above_diffuse_white` (past white).

## Two modes, deliberately separate

Settled 2026-09-10 with the user. There are **two chart modes**, and they are not
one design with a parameter:

- **Compare (n variants).** Several configs at once. This is the app's premise,
  and it is what ranked the percentile curve and the a\*/b\* path above the rest
  in the 2026-09-03 discussion (recorded in `docs/progress/analysis.md` under
  `metrics-visualization`).
- **Inspect (one variant).** A single config, spending the freed room on detail
  a comparison cannot afford — per-channel histograms, per-vertex annotation.

The rule for which mode a chart belongs to: **a chart can take n variants only if
it still has a free series dimension.** Charts that already spend it on something
else cannot. Per-channel histograms spend it on R/G/B; the hue polar spends it on
angle. That is a property of the encoding, not a preference.

Two consequences worth keeping:

- **An n-variant chart at n=1 is its own design, not just fewer lines.** The
  legend goes (a single series is named by the title), any difference strip has
  nothing to compare, direct labels turn redundant — and the freed room should be
  spent. Draw the n=1 case explicitly rather than assuming graceful degradation.
- **The two modes want opposite things from the config toggle**, which also
  settles an open question in the integration half: compare-mode charts draw
  every config and the toggle *emphasises* one (so nothing moves, matching the
  picture's promise), while inspect-mode charts are bound to the active config
  and swap in place exactly as the picture does.

Beyond three overlaid configs, colour alone stops separating them — measured:
the `dataviz` reference dark steps pass all-pairs CVD at two and three series and
fail at five. Past three, compare mode has to become small multiples.

## Settled — the v1 component set

Decided 2026-09-11 with the user, against real records on the design canvas.
**Three charts ship.** Everything else is deferred with a reason, so a later
reader can tell a rejection from an omission.

| # | Chart | Mode | Source |
| --- | --- | --- | --- |
| 1 | **Luminance histogram** | both | `tone.histogram.series.luminance` |
| 2 | **Per-channel histogram** (R/G/B + luminance) | inspect | `tone.histogram.series.*` |
| 3 | **Cast over tone — two axis-coloured curves** | inspect | `color.cast_by_tone_band` |

**Chart 3's shape, because it took several attempts.** x is the tone band, dark
to light; y is CIELAB with **0 drawn as neutral**; two lines, `a*` and `b*`. Each
line is **coloured by its own value** — `a*` runs green below zero to red above,
`b*` blue below to yellow above — so the line teaches its own axis and nothing
has to be memorised. Implemented as a vertical SVG gradient in user space, so
stroke colour is a function of y position, which *is* the value.

Three things that are easy to get wrong here:

- **Colour is redundant with position.** The ramp is fixed, not scaled to the
  data, or a mild cast on one frame and a severe one on another would look alike.
  Read the value off the axis, never off the hue.
- **Ramp ends sit at the sRGB gamut limit for their direction, measured.** At
  L\* 65 green clips at 42.8 and blue at 54.3, and green is what caps the `a*`
  ramp — hence ±41 for `a*` and ±52 for `b*`, not one number for both. Push past
  those and the hue skews instead of saturating.
- **A mark and the line under it must use one mapping.** Painting marks with the
  *true* value's colour while the line gradient spans the axis range makes a
  marker look more muted than its own stroke, worst at the extremes.

**Rendering: inline SVG, hand-rolled.** Confirmed rather than assumed — every
artboard on the canvas is hand-written SVG and none of it wanted a library. The
datasets are small (200 bins, 7 bands), labels must stay crisp, and since PR #108
every colour and size has to come from a Panda token under `strictTokens`, which a
charting library's own CSS would sit outside of.

**Units: L\* and fractions, never stops.** The histogram is stored one bin per
L\* unit and the bands are cut in the same domain, so the two compose exactly.
No shipping v1 chart uses the stops domain.

**Component decomposition**, as the canvas actually factored: a plot frame
(margins, recessive grid, axis labels); a band axis; reference lines the record
names rather than the chart deriving (`mid_grey_bin`, `diffuse_white_bin`); a
legend; and **two separate colour systems** — see the constraint below.

**The constraint that splits the modes.** Once colour carries hue it cannot also
carry config identity. Chart 3 is therefore inspect-mode *by construction*: at
n ≥ 2 it would need dash or shape to separate conversions and the colour
advantage would go. Compare mode spends colour on the config and reads cast from
position alone.

**A band's population must reach the mark.** `cast_by_tone_band` carries `pixels`
and `sparse`; a sparse band is drawn hollow, never at the weight of one holding
half the frame. On the old cut the largest excursion rested on **one pixel of
15.1 million**.

## Deferred, with reasons

- **Percentile curve** — not shipping. It was ranked first before a histogram
  existed in the record; on one frame it is hard to read, because density only
  shows as the curve going flat. Its one surviving claim is that it turns a
  difference into a *number*, which v1 does not need. The canvas keeps it as the
  record of why.
- **The a\*/b\* path** — rejected. Crossover as the shape of a 2-D trajectory is
  real but too hard to read; tone is only implied by vertex order.
- **Chroma as a third curve** — rejected. It is `hypot(a*, b*)`, derivable from
  the two lines that ship, and it crowded the panel.
- **`hue_sectors`** — deferred. Six sectors is coarse, and polar charts compare
  poorly, which matters in a comparison tool.
- **`endpoints`** — deferred as a *conditional* strip: all-zero is the healthy
  case, and a permanent panel of empty bars trains the eye to skip it. Worth
  building when something is non-zero, as with the blue-only top-code population
  it once caught.
- **`tone.bands` stacked bar** — deferred, but its rejection no longer holds. The
  L\* re-cut took frame G2's largest band, across its five presets, from 93.8% to
  51.7%, and it now separates the preset families by eye. (Across all 33 renders
  the recorded figures are a median of 82.6% → 46.3% and a worst of 95.0% →
  56.2%; see `analysis/conversion-metrics` in the progress log.) A strong v2 candidate, and the natural roll view.

## Still open

- Whether chart 3's x axis stays evenly-spaced band slots or moves to true L\*
  centres, which is what would let bands, histogram and cast share one axis.
- What a compare-mode cast chart looks like, given colour is spent on config
  identity there.
- Degenerate cases beyond `sparse`: two configs whose curves nearly coincide, and
  a measurement absent entirely.

## Non-goals

No new measurements. No verdicts — it describes, it does not rank, the same rule
the measurement side holds to. Not the app wiring, which is the other half.

## How to Verify

The charts are drawn from **real** records (two configs of the same frame, via
`scripts/preset-review/generate.py` plus `nctool metrics`), not invented numbers,
and a reader can go from a visible feature back to the field that produced it.
The encodings survive the degenerate cases above. The outcome is a decision
recorded here and in the progress log, concrete enough for the integration half
to build against.

## Dependencies

- [Conversion Metrics & Photographic Analysis](conversion-metrics.md)
