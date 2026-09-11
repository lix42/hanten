# Metrics Visualization

## Goal

Put the metrics charts **inside the existing review app**, so that numeric review
sits beside visual review instead of in a separate tool. Today `nctool metrics`
emits JSON and a Markdown table; both answer "what changed" well and "what does
that look like" not at all.

**The charts themselves are [Metrics chart design](metrics-chart-design.md)** —
split out on 2026-09-10 because deciding the encodings is the harder half and
does not depend on the app. This task is the wiring.

## What exists

- **`nctool metrics {image,roll,table}`** (`analysis/conversion-metrics`) — the
  data source. One record per (image, config). At `schema_version` 2 it gained
  `tone.histogram`, which is the first **drawable** field any of these records
  has carried and roughly doubles the record's size — a transport consideration,
  not just a charting one. The shape itself is described in
  [Metrics chart design](metrics-chart-design.md).
- **`tools/review-app`** (`analysis/comparison-review-tooling`, viewer shipped
  2026-09-02, fullstack since 2026-09-10; TanStack Start on Vite+ / Solid /
  Panda CSS) compares configs by toggling renditions **in place**: every
  rendition of an image shares one grid cell, so switching config cannot move
  the picture. Its `review.json` is `configs × images → renditions`, and
  `SCHEMA.md` already calls `images[].note` "the natural home for measured
  numbers" — a hook `scripts/preset-review/generate.py` now uses for G/R and B/R
  means.

Those two shapes line up: a metrics record is per (image, config), which is
exactly what `renditions` is keyed by.

## Open questions

- **How does a record reach the app?** A new key on `renditions[config]`, a
  sibling file, or read from `nctool` output directly. Inlining keeps a review
  set self-contained (the schema's stated virtue) but duplicates data; a
  reference keeps one copy but breaks "move the directory anywhere". The app now
  has a **server** that reads the set from disk and watches it, so a sibling file
  is cheaper than it was — but it has to join the watch targets, or it goes stale
  exactly the way renditions did (mtime frozen at parse time, no error).
  Note `parseReview` rejects an unknown *config id* but silently ignores unknown
  top-level and image-level keys, so an inlined block would be accepted and
  unvalidated.
- **Do the charts toggle in place like the image, or sit beside it?** In place is
  consistent with the app's core interaction; an overlay of both configs at once
  is strictly more informative for a curve. Those pull in opposite directions and
  the answer may differ per chart.
- **What does a config with no measurement render as?** The schema says a missing
  *rendition* is fine and an unknown config id is an error; charts need their own
  answer.
- **Per-frame and per-roll are different views.** Does the app grow a roll view,
  or does the roll rollup stay in Markdown?

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

- [Metrics chart design](metrics-chart-design.md)
- [Comparison review tooling](comparison-review-tooling.md)
