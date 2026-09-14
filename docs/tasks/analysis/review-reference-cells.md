# Reference cells in the review set

## Goal

Let a review set place an **outside producer's image** beside nc's renders of the same
frame: a Negative Lab Pro export, a SmartConvert TIFF, or the user's own hand-tweaked
target. Today every cell of a `nctool review generate` matrix is an `nc convert`, so
"how does nc compare with what I actually wanted" cannot be put in front of the eye.

## Why it is its own task

Three tasks asked for this and none owns it: `film-base/dmax-per-channel-reduction`'s
parking note (2026-09-13) names it as the right home for its unshipped scale review;
`analysis/nlp-comparison` asks whether the side-by-side belongs there or here; and
`algo/contrast-latitude-spike` wants any option offered for a taste judgement
"rendered into a review set rather than argued numerically", which for a
contrast comparison means NLP in the grid.

## What is known

- The grid already toggles renditions of one frame in place, so a reference cell needs
  no new viewer mechanics: it is one more rendition keyed by config name.
- References differ from renders in encoding and framing: NLP exports are 32-bit float
  linear sRGB or 16-bit Adobe RGB (the `2026-09-09` Ektar batch), and some are cropped
  to a different aspect. A cell has to be brought to a **common SDR sRGB JPEG** or the
  comparison is confounded by nc decoding as HDR beside NLP's SDR. Cropped references
  are shown as they are; nothing resamples one side to match the other (the
  `nlp-comparison` rule).
- Pairing is by manifest `source_frame`, never by registration (decided 2026-09-02).
- Each cell may carry a `nctool metrics` record; for a reference cell that is
  `metrics image --space <declared>` on the reference file, so the charts appear for it
  too.
- `sips` destroys a gain map when downscaling, which is why HDR review is deferred;
  reference cells are SDR and do not meet that blocker.

## Open questions

1. Matrix syntax: a cell kind (`{"reference": "<path>", "space": "adobe-rgb"}`) beside
   the `nc convert` cells, or a separate list resolved through the manifest by
   `source_frame`?
2. Who converts the reference to the common JPEG, the generator or the server? The
   generator writes files beside the set today; the server only serves them.
3. Does a reference cell get the `reference` / `target` role distinction
   `nlp-comparison` defines, and does the app show it?

## How to Verify

- A matrix with one reference per frame renders a set where the reference toggles in
  place with nc's renders, at the same grid position, with its declared space recorded.
- A reference with no matching `source_frame` is reported, not silently dropped.
- Re-running the generator is idempotent, and the review set stays outside the repo.

## Dependencies

- [Comparison review tooling](comparison-review-tooling.md) — the generator and app
  this extends.
