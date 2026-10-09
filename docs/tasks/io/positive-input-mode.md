# Negatives scanned in positive mode

## Goal

Convert a colour negative scanned in SilverFast's **positive** mode (`Negative=No`,
usually with the scanner's IT8 profile embedded), as some people prefer to scan. Until
this task `io/input-data-semantics` refused every `Negative=No` scan with exit 4.

## Why it is its own task

`io/input-data-semantics` deferred positive mode "to file formally". The asset set holds
a roll scanned this way (`rolls/Portra4000-2026-08-05-positive/`). This task was first
written as support for an already-positive image; measuring that roll showed its pixels
are a negative, so slide film moved to `io/slide-film-input` (low priority).

## What is known

- In HDR/HDRi raw mode a positive-mode scan holds the same linear scanner transmission
  as a negative-mode scan. The asset roll holds twin scans of the same frames in both
  modes (`Negative=No` 1255-1288, `Negative=Yes` 1293-1325): the unexposed twins'
  bases agree within 0.7%, and `measure-roll` over matched frames agrees to about
  0.02 EV and 1.5% on its gains (figures in the progress log).
- The embedded profile (`SFprofT (OpticFilm 8300i)`) is the scanner's slide
  calibration with gamma-2.2 TRCs, so it does not describe the linear data; it stays
  recorded and unapplied, like any embedded ICC before density.
- Metadata cannot tell this scan from a slide. A content check stands in: a negative's
  base is its most transparent film, so a frame with much of its area above the base
  is warned about (a slide, or a wrong base).

## How to Verify

- The positive roll converts through the default destination and an HDR one, with
  the tag in `input_color.evidence` and no polarity warning on its picture frames.
- A frame far brighter than its base warns that it does not look like a negative.
- Negative scans are byte-identical through decode and convert.

## Dependencies

- [Input data semantics and validation](input-data-semantics.md) — the detection and
  the meaning model this builds on.
- [Film-master and shared display pipeline](../color/film-master-render-pipeline.md) —
  the shared display path the conversion runs through.
