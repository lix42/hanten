# Metrics Visualization

## Goal

Make the measurements readable as pictures, **inside the existing review app**,
so that numeric review sits beside visual review instead of in a separate tool.
Today `nctool metrics` emits JSON and a Markdown table; both answer "what
changed" well and "what does that look like" not at all.

## What exists

- **`nctool metrics {image,roll,table}`** (`analysis/conversion-metrics`, done
  2026-09-03) writes a per-image record — endpoint occupancy, tone in log2 stops,
  colour in CIELAB — and a per-roll rollup with a spread table. That is the data
  source; this task does not add measurements.
  **Two things changed under it on 2026-09-10 (`schema_version` 2) and both matter
  here.** `tone.histogram` arrived: four series (luminance, R, G, B) of 200 counts
  each, one per L\* unit, running to twice diffuse white so an HDR render's
  headroom is drawable and an SDR render's shortfall below white is visible; the
  record names the `mid_grey_bin` and `diffuse_white_bin` reference lines. It is
  the first drawable field in any of these records, and it was added for this
  task — the **luminance** series is the primary single-frame view. And the bands
  were re-cut in the same L\* domain, so **every band edge falls exactly on a
  histogram bin edge**: bands and bars share one axis and can be drawn without
  interpolating. The re-cut is also what makes item 3 below worth drawing at all —
  on the old stops-even edges one band held a median 83% of a frame, so a stacked
  bar was one block.
- **`tools/review-app`** (`analysis/comparison-review-tooling`, viewer half
  shipped on `main` 2026-09-02, fullstack since 2026-09-10; TanStack Start on
  Vite+ / Solid / Panda CSS) compares configs by toggling renditions **in place**:
  every rendition of an image shares one grid cell, so switching config cannot
  move the picture. Its `review.json` is `configs × images → renditions`, and
  `SCHEMA.md` already calls `images[].note` "the natural home for measured
  numbers" — a hook nobody has used yet.
  Note it now has a **server**: it reads the set from disk by path and watches
  it, so metrics JSON could be read server-side beside the review file rather
  than fetched, and would re-read on change for free. Whether metrics belong in
  `review.json`, in a sibling file, or are read from `nctool` output directly is
  still open — but the choice is now wider than it was when this was written.

Those two shapes line up: a metrics record is per (image, config), which is
exactly what `renditions` is keyed by.

## What the design discussion settled (2026-09-03)

Ranked by value, with the reasoning, because the ordering is the useful part:

1. **`percentiles_stops` as a curve** — the highest-value plot and the place to
   start. It contains what `bands` contains and more, and **two overlaid answer
   the question the app exists for**: vertical gap is exposure, relative slope is
   contrast, and where they diverge says whether shadows, midtones or highlights
   moved. Annotating the ends gives `toe_span` / `shoulder_span` for free — a
   curve going horizontal at the top *is* `shoulder_span: 0`.
2. **`cast_by_tone_band` as a path on the a\*/b\* plane**, one point per band
   joined in tone order. The shape of that path is crossover: a tight cluster is
   clean, a long diagonal sweep is not. As a bar chart the same data is nearly
   unreadable. Seven bands since 2026-09-10, and a band holding under 0.1% of
   the region is flagged `sparse`: its point must not be drawn with the same
   weight as one from a band holding half the frame.
3. **`bands` as a stacked bar** — redundant with (1) for a single image, but the
   right way to show a whole roll at once, one bar per frame.
4. **`endpoints` as a small per-channel bar.** Unglamorous, but it is the closest
   thing here to a correctness signal, and on a real frame it showed a 22%
   top-code population that was *blue only* — which no other view made obvious.
5. **`hue_sectors`, last.** Six sectors is coarse and the natural polar rendering
   invites over-reading; two polar charts also compare poorly, which matters in a
   comparison tool.

Scalars (`key_stops`, `contrast.p95_minus_p5`, `mean_cast`, `b_over_g`) belong in
a readout strip, not a chart.

**The constraint that shapes all of it: design every chart to overlay two configs
from the start**, even if the first version renders one. That is what ranks the
curve and the a\*b\* path above the others, and it is the app's whole premise.

## Open questions

- How does a metrics record reach the app? A new key on `renditions[config]`, a
  sibling file the page fetches, or inlined axes in `review.json`? Inlining keeps
  a review set self-contained (the schema's stated virtue) but duplicates data
  that already exists; a reference keeps one copy but breaks "move the directory
  anywhere".
- Do the charts **toggle in place** like the image does, or sit beside it? In
  place is consistent with the app's core interaction, but an overlay of both
  configs at once is strictly more informative for a curve — those pull in
  opposite directions and the answer may differ per chart.
- What does a config with no measurement render as? The schema already says a
  missing *rendition* is fine and an unknown config id is an error; charts need
  their own answer.
- Per-frame and per-roll are different views. Does the app grow a roll view, or
  does the roll rollup stay in Markdown?
- Hand-rolled SVG or a charting library? The app has no chart dependency today
  and this repo has a habit of hand-rolled SVG/CSS, but overlays with axes and
  hit-testing are where that habit stops paying.
- Percentages and units: the JSON keeps fractions deliberately, and the Markdown
  report converts. A chart needs the same decision made once.

## Non-goals

Not a new application, not a replacement for the Markdown report (which stays the
committable, diffable artifact), and no verdicts — the same rule the measurement
side holds to: it describes, it does not rank.

## How to Verify

- A review set carrying measurements renders its charts, and one without them
  still loads — the app must not require the data it did not have yesterday.
- Switching config updates the charts and the picture together, without either
  moving on screen.
- A chart's numbers agree with the record it came from; a reader can go from a
  visible feature to the field that produced it.
- The app's own gates (`pnpm verify`) stay green, and a set with a partially
  measured config is handled rather than crashing.

## Dependencies

- [Conversion Metrics & Photographic Analysis](conversion-metrics.md)
- [Comparison review tooling](comparison-review-tooling.md)
