# Positive-mode and ICC-embedded input

## Goal

Accept a SilverFast scan that is already a positive (slide film, or a negative the
scanner inverted) and carry it to the display outputs without negative
reconstruction. Today `io/input-data-semantics` detects `Negative=No`
(`DecodeInfo::is_silverfast_positive_mode`) and rejects it loudly with exit 4, and an
embedded ICC (TIFF tag 34675) is extracted and recorded but never applied.

## Why it is its own task

`io/input-data-semantics` deferred it as a follow-up "to file formally" and never did.
The asset set already carries a positive roll
(`rolls/Portra4000-2026-08-05-positive/`, 18.66 MP HDRi with an IR plane), so the
input exists and is refused today.

## What is known

- The detection and the rejection are in place; this task turns the rejection into a
  path. The provenance rule stands: the file's own metadata decides, never a flag.
- A positive has no film base to divide by and no density curve to invert, so
  `calibration.film_base` (required today for `convert`) and the whole `reconstruction`
  object do not apply. The pipeline would enter at the working space: the embedded
  ICC (or a declared `--input-meaning colorimetric` reference) gives the pixels
  colorimetric meaning, which `input_semantics` already models as
  `MeasurementMeaning::Colorimetric` and marks "recognized but unsupported: no inverse".
- From linear ACEScg onward the shared print controls and every display preset apply
  unchanged; `film-master` would be the positive's own colorimetry, not a film
  rendering, and its report must say so.
- A positive-mode scan may still carry a usable IR plane; the holder mask is
  irrelevant (no base to measure) but IR dust removal, when it exists, is not.

## Open questions

1. What does the recipe look like for a positive? Refusing `reconstruction` and
   `film_base` keys, or accepting and ignoring them with a warning? The project rule
   is that an ignored knob is a bug, so refusal is the default answer.
2. Is the ICC applied through lcms2 into ACEScg directly, or through a declared
   linear RGB space first? `color/scanner-profile-before-density-experiment` asks a
   related question for negatives; do not conflate the two.
3. Memory: the decode and film-base phases change shape (no film-base pass, one lcms2
   transform on the whole frame). `pipeline::memory` needs its own `RunProfile` row.
4. What `hanten inspect` and `estimate` do on a positive.

## How to Verify

- The positive roll converts through `display-p3` and `hdr-linear-tiff` with a report
  that states the input meaning, the ICC identity, and that no reconstruction ran.
- A positive with no embedded ICC and no declared meaning is refused with a message
  naming `--input-meaning`.
- Negative scans are byte-identical through decode and convert.
- The memory profile is calibrated on two frame sizes before the path is enabled.

## Dependencies

- [Input data semantics and validation](input-data-semantics.md) — the detection and
  the meaning model this builds on.
- [Film-master and shared display pipeline](../color/film-master-render-pipeline.md) —
  the ACEScg entry point and shared controls the positive joins.
