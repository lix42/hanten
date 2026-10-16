---
title: Split a two-half-frame file
status: todo
deps: [core/crop-rotate, core/frame-keys-by-path]
summary: one file holding two half-frames becomes two pictures, each its own frame
---
# Split a two-half-frame file

## Goal

A scanner treats a half-frame roll as 135, so one file holds two half-frames. Split it
into two pictures, each converted and overridden as its own frame; a half-frame roll may
then show up to 80 pictures ([desktop app design](../../design/desktop-app.md) §2.4).
A flag and recipe key first, as with every knob.

## What is known

- Crop and rotate come first (`core/crop-rotate`); a split is two crops of
  one file.
- Each half needs its own `roll.frames` key (`core/frame-keys-by-path`).
- The pricing cap counts scan files, not pictures, so a split file counts once
  (desktop app design §2.11).

## Open questions

1. Where the split line is: the middle, found from the frame gap, or stated.
2. Whether each half is measured on its own for the roll's pooled measurements.
3. How output files are named.

## How to Verify

- A real half-frame scan splits into two outputs whose reports each name their half.
- A file without a split is byte-identical.
