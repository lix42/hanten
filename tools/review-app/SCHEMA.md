# The review-set format

A **review set** is one `review.json` plus the images it names. The server is
pointed at the `review.json` (`pnpm dev <path>`, or `REVIEW_SET`) and resolves
every image path **relative to that file**, so a set is a self-contained
directory you can move or copy anywhere. It does not have to sit beside the app,
and nothing about it needs to be reachable from a served root.

Keys are `snake_case`, matching nc's own reports and recipes — the producers are
nc-adjacent scripts, not JavaScript.

## Shape

```json
{
  "schema_version": 1,
  "title": "Display tone: shoulder vs none",
  "description": "Same reconstruction on both; only the display stage differs.",

  "configs": [
    { "id": "shoulder", "label": "shoulder", "note": "shipped Hermite shoulder" },
    { "id": "none", "label": "none", "note": "--display-tone none" }
  ],

  "images": [
    {
      "id": "E1",
      "label": "E1 — Ektar 100 · 20260713-nikon-971.tif",
      "note": "blown 6.86% → 5.65% · code sep 11.6 → 15.2",
      "renditions": {
        "shoulder": "E1-shoulder.jpg",
        "none": {
          "src": "E1-none.jpg",
          "preview": "E1-none-thumb.jpg",
          "metrics": "E1-none.jpg.metrics.json"
        }
      }
    }
  ]
}
```

## Fields

| Field                  | Required | Meaning                                                              |
| ---------------------- | -------- | -------------------------------------------------------------------- |
| `schema_version`       | yes      | Must be `1`. A future format bumps it rather than changing this one. |
| `title`, `description` | no       | Shown above the first image.                                         |
| `configs[].id`         | yes      | Referenced by `renditions`. Must be unique.                          |
| `configs[].label`      | yes      | Button text. Keep it short — it sits in the top bar.                 |
| `configs[].note`       | no       | Tooltip on the button.                                               |
| `images[].id`          | yes      | Used as the label when none is given.                                |
| `images[].label`       | no       | Heading for the section.                                             |
| `images[].note`        | no       | A line beside the heading — the natural home for measured numbers.   |
| `images[].renditions`  | yes      | Object keyed by **config id**.                                       |

A rendition is either a **string** (the image path — the common case) or an
object:

| Field             | Required | Meaning                                                                                                                                                                                                                                                                                                                                                                                             |
| ----------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src`             | yes      | Path to the full image, relative to `review.json`.                                                                                                                                                                                                                                                                                                                                                  |
| `preview`         | no       | Thumbnail path. Defaults to `src`.                                                                                                                                                                                                                                                                                                                                                                  |
| `metrics`         | no       | Path to this rendition's `nctool metrics` record, relative to `review.json`. The app draws its tone and cast charts beside the picture. See below.                                                                                                                                                                                                                                                  |
| `width`, `height` | no       | Natural size in pixels; `nctool review generate` states them for every rendition it measures. Lets the page reserve space before the image loads — it never scales the picture, and `fullsize` always shows the file at 1:1. State them correctly or leave them out: wrong numbers mis-size the reserved box and the mini-map, and the app logs a console warning when they disagree with the file. |

## Two rules worth knowing

**Order matters.** `configs` order sets both the button order and the keyboard
mapping: `1`–`9` select the first nine, `0` selects the tenth. Past ten there is
no key, but the buttons still work.

**An unknown config id is an error; a missing rendition is not.** A rendition
keyed by a config that was never declared is a typo the author wants to hear
about, so the whole set is refused with a message naming it — the same reason
every recipe struct in nc uses `deny_unknown_fields`. A config that simply has no
rendition for one image is ordinary (it failed to render, or the frame was added
later), so that slot renders as a visible gap and the rest of the set still
loads. A comparison silently missing half of itself is the worst outcome of the
three.

**A rendition may name a file that is not there yet**, and that is not an error
either — a render that failed, or one still to come. Its slot renders as a gap
and the request for it returns 404 naming the missing file. It starts working the
moment the file appears: the server watches the set, so a re-render updates the
page in place without a reload.

## Measurements

A rendition may name the **metric record** of its own pixels — what
`python -m nctool metrics image` writes, and what
`nctool review generate` writes beside each rendition it renders. It is a
sibling file rather than a block in this document: it is ~20 kB of histogram
counts per rendition, a different tool writes it at a different time, and
keeping it separate is what lets a re-measurement update the page without
rewriting the review file.

The server reads it, keeps the part the charts draw, and **watches it** exactly
as it watches the images — so re-measuring updates the page in place, the same
way re-rendering does.

Three rules, following the ones above:

- **A rendition naming no record is ordinary.** It renders its picture and says
  it has no measurement. A set written before measurements existed, or rendered
  with `--no-metrics`, still loads.
- **A record that cannot be read costs only its own charts.** Missing, truncated,
  or from a schema older than the charts need: the page says so where the charts
  would be, and every other config of the comparison is unaffected. Refusing the
  whole set over one unreadable record would take the pictures down with it.
- **The record must describe _that_ rendition.** Nothing checks this — a record
  measured from a different file would chart happily. `nctool review generate`
  writes each one beside the image it measured and keys reuse to that image's
  checksum, which is what keeps the pairing honest.
