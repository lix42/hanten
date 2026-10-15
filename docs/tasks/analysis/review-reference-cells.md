---
title: Reference cells in the review set
status: done
deps: [analysis/comparison-review-tooling]
tags: [post-mvp]
summary: a SilverFast, SilverFast CCR or NLP export as a grid cell beside Hanten's renders, made by the generator
done: 2026-10-14
---
# Reference cells in the review set

## Goal

Let `nctool review generate` place an **outside producer's export** beside Hanten's
renders of the same frame. The producers that matter are SilverFast (with and
without CCR) and Negative Lab Pro. SilverFast is often the better of the two, and
NLP is there because it is popular. SmartConvert is not worth comparing.

Before this, every cell of a matrix was a `hanten convert`, so outside exports were
merged into `review.json` by hand: sips conversions, a one-off `build_review.py`
(`../temp/gold200-1012-roll-review/`).

Comparing against images the user edited by hand is out of scope. This task only
gets external images into the review app through the tool.

## Why it is its own task

Three tasks asked for it and none owned it: `film-base/dmax-per-channel-reduction`'s
parked NLP review, `analysis/nlp-comparison`'s open question on where the
side-by-side belongs, and `algo/contrast-latitude-spike`'s "render it into a review
set rather than argue numerically".

## What is known

- The app needs no change. A reference is one more config whose `producer` is
  `kind: "external"` (`tools/review-app/SCHEMA.md`).
- The manifest already pairs exports to scans: `converted/<producer>/<roll>/` entries
  carry `source_frame`. Some NLP batches have it `null` on most entries.
- Exports differ in encoding: 16-bit Adobe RGB (most), 32-bit float linear sRGB (the
  early NLP batches), and SilverFast's `2026-09-20-Portra400` with no profile at all.
  Several carry v2 profiles (no `chad`), whose colorants are D50-adapted.
- Pairing is by `source_frame`, never by registration (decided 2026-09-02). Cropped
  exports are shown as they are and never resampled.

## Decided

1. **Matrix syntax:** a top-level `references` list naming a manifest producer, with
   an optional `space` for an export that has no profile. A path in the matrix would
   bypass the manifest's pairing.
2. **The generator converts** to a common SDR sRGB JPEG. The server only serves
   files. The space comes from the export's embedded ICC profile, and a declared
   `space` may only fill in where there is no profile.
3. **No roles.** There is no `reference` / `target` distinction here.
4. **Hanten's cells must be SDR sRGB JPEG** when a matrix has references.
5. **Charts only where the crop matches** the scan's dimensions.

## How to Verify

- A matrix with references renders a set where each export toggles in place with
  Hanten's render, and its colour space and where that came from are recorded.
- A frame with no export, and an export with no `source_frame`, are both reported
  rather than silently dropped.
- Re-running the generator is idempotent, and the review set stays outside the repo.
- The colour conversion agrees with an independent decode (`sips -m`) on each
  encoding.

## Dependencies

- [Comparison review tooling](comparison-review-tooling.md): the generator and app
  this extends.
