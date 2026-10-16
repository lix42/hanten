---
title: Frame offsets on multiplicative knobs
status: todo
deps: []
summary: is a frame's contrast or saturation offset a ratio or a step? settled by a review round
---
# Frame offsets on multiplicative knobs

## Goal

A frame override is an offset on the roll ([desktop app design](../../design/desktop-app.md) §2.3). For exposure that is a sum in EV;
contrast and saturation multiply a slope, so a frame's offset could be a ratio or a step
on some scale. The colour controls are per-channel, and tier-3 switches and named steps
have no offset at all. Settle what an override means for each kind of knob, before the
project file format is fixed.

## What is known

- `--contrast` multiplies the frame's **base slope**, which is its placed slope, else a
  thin frame's slope, else the roll's; `--saturation` multiplies the colour's base slope
  (`docs/using-nc.md`). So frames on one roll start from different slopes.
- An offset must follow the roll: when the roll's value changes, the frame keeps its
  difference.

## Open questions

1. Ratio or step for contrast and saturation, judged on real renders where the roll's
   value moves, including placed and thin frames.
2. A per-channel offset for overall and tone cast, and a plain override for tier-3
   switches and named steps: confirm, and define lift and reset for each.
3. How the UI shows a frame's offset next to the roll's value.

## How to Verify

- A review round (`render-review-set`, at most four arms) on a real roll, and the
  decision recorded in the design doc.
