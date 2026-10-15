---
title: Frame offsets on multiplicative knobs
status: todo
deps: []
summary: is a frame's contrast or saturation offset a ratio or a step? settled by a review round
---
# Frame offsets on multiplicative knobs

## Goal

A frame override is an offset on the roll ([desktop app design](../../design/desktop-app.md) §2.3). For exposure that is a sum in EV;
contrast and saturation multiply the roll's slope, so a frame's offset could be a ratio
or a step on some scale. Settle which reads right, before the project file format is fixed.

## What is known

- `--contrast` multiplies the roll's slope; `--saturation` multiplies the colour's base
  slope (`docs/using-nc.md`).
- An offset must follow the roll: when the roll's value changes, the frame keeps its
  difference.

## Open questions

1. Ratio or step, per knob, judged on real renders where the roll's value moves.
2. How the UI shows a frame's offset next to the roll's value.

## How to Verify

- A review round (`render-review-set`, at most four arms) on a real roll, and the
  decision recorded in the design doc.
