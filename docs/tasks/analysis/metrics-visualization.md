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
  numbers" — a hook the generator uses for G/R and B/R means.

Those two shapes line up: a metrics record is per (image, config), which is
exactly what `renditions` is keyed by.

## Settled

All four, on 2026-09-12, while wiring it:

- **A record reaches the app as a sibling file**, named by an optional `metrics` key on a
  rendition (`tools/review-app/SCHEMA.md`). Not inlined: it is ~20 kB of histogram counts per
  rendition, written by a different tool at a different time, and a separate file is what
  lets a re-measurement update the page without rewriting the review document. It is read and
  parsed **server-side**, so what crosses the wire is the charted subset rather than the whole
  record. `schema_version` stays 1 — the key is additive and optional, so an older build
  ignores it rather than refusing the set.
  **It joins the watch targets**, stamped beside the renditions: the model holds the *parsed*
  record, so without that a re-measurement would sit invisible behind the previous numbers,
  exactly the way renditions once did.
- **The charts swap in place with the config**, below the picture, not beside it and not
  overlaid. All three v1 charts spend colour on what they encode, so none has a series
  dimension left for a second configuration — comparing is switching config and watching the
  shape change, the same gesture the picture already asks for. Below rather than beside
  because the stage is the widest thing on the page and must not be narrowed.
- **A config with no measurement renders its picture and says so.** A record that cannot be
  read says why, where the charts would be. Neither refuses the set: the picture is what this
  app exists to show, and one unreadable record must not take four good comparisons with it.
- **Per-roll stays in Markdown.** The app is per frame; `nctool metrics roll` already rolls
  the scalars up, and a roll view is a different question from this one.

Two things worth knowing for whatever comes next:

- The charts describe the **measured region**, not the frame — a set insets its measurement
  so the film holder and rebate stay out of the statistics, so the panel states what share of
  the frame it covers. A holder that the inset fails to clear shows up as a hard spike at the
  bottom of the L\* axis.
- Nothing checks that a record describes the rendition it is attached to. The generator
  writes each one beside the image it measured and keys reuse to that image's checksum; a
  hand-assembled set could pair them wrongly and nothing would notice.

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
