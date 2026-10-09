# Slide film input

> **Low priority** (user, 2026-10-08): there are no slide scans to develop or verify
> against. Pick it up when one exists.

## Goal

Convert a scan of **positive (slide / E-6) film**: the picture is already a positive,
so there is no film base to divide by and no density curve to invert. The frame
enters the chain at the working space and gets the same scene correction and
destinations as a negative.

## Why it is its own task

`io/positive-input-mode` was first written as this task, but the roll it pointed at
is a colour **negative** scanned in SilverFast's positive mode, so that task now
covers that case only. Slide support is feasible but is not the product's focus, and
nothing in `nc-assets` exercises it.

## What is known

- A SilverFast raw (HDR/HDRi) positive-mode scan of a slide carries the same tags as
  a negative scanned in positive mode (`Negative=No`, linear `Gamma=1`). Metadata
  cannot tell them apart; `io/positive-input-mode`'s check (does the picture look like
  a negative?) is the signal to build on.
- Positive-mode scans embed the scanner's SilverFast IT8 profile (`SFprofT (OpticFilm
  8300i)` on the asset roll): an input-class profile measured from a **transparency**
  target, so it is the natural scanner → colorimetric characterization for slides. Its
  TRCs are gamma 2.2 while the raw data is linear, so how it applies to raw samples
  is unverified.
- The new chain's entry point for a positive is settled: an `AcesCgImage` ahead of
  scene correction (`nf-scene-correction/stage`).

## Open questions

1. How the embedded profile applies to linear raw samples (encode to its TRC first,
   or something else), checked on a real slide scan with a known target if possible.
2. How a slide is identified or declared, given the metadata cannot do it.
3. What the recipe looks like: `calibration.film_base` and the decode do not apply,
   and an ignored knob is a bug.
4. What `inspect`, `measure-base` and `measure-roll` do on a slide.

## How to Verify

- A real slide scan converts to the default destination and an HDR one, with a
  report that states no negative decode ran and which profile was applied.
- Negative scans are byte-identical through decode and convert.

## Dependencies

- [Negatives scanned in positive mode](positive-input-mode.md) — the positive-mode
  acceptance and the "does not look like a negative" check.
- [Scene correction as a named stage](../nf-scene-correction/stage.md) — the
  working-space entry point.
