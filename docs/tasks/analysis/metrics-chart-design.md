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

## Open questions

- **Encoding per measurement.** `cast_by_tone_band` is the sharpest case: one
  path on the a\*/b\* plane (tone implicit in path order — shows crossover shape,
  hides which band), two curves against a tone axis (tone explicit, 4 lines at
  two configs), or three with chroma. The 2026-09-03 ranking argues for the
  path; all three are now drawn on real data in the design canvas.
- **Whatever the encoding, the mark has to carry the band's population.** Measured
  2026-09-10: the `highlight` vertex was the largest excursion in every colour
  encoding and rested on **one pixel of 15.1 million**. At equal visual weight it
  invents a crossover out of rounding. The re-cut reduced but did not remove this
  — hence the `sparse` field, which a chart must read rather than ignore.
- **The re-cut is what makes a stacked band bar worth drawing at all.** On the old
  stops-even edges one band held a median 83% of a frame, so the bar was one
  block; that was the evidence for demoting it, and it no longer applies.
- **How many visuals.** Five groups need not mean five charts. `percentiles_stops`
  and `bands` describe the same axis and might share it; `hue_sectors` and
  `cast_by_tone_band` are both colour-over-tone. What genuinely needs its own frame?
- **Rendering technology.** Inline SVG, canvas, or something else. The lean is
  SVG: the datasets are tiny (11 / 5 / 6 points), labels must stay crisp,
  hit-testing matters for "which band is this", the app server-renders, and since
  PR #108 every colour and size must come from a Panda token under `strictTokens`
  — which a charting library's own CSS would sit outside of.
- **Component decomposition.** What is shared: a plot frame, a stops axis, a
  legend, the two-config overlay pair.
- **Units.** The JSON keeps fractions deliberately; the Markdown report converts.
  A chart needs that decided once.
- **Degenerate cases**, which are where an encoding quietly fails: two configs
  whose curves nearly coincide, a band with no pixels, a measurement absent
  entirely.

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
