---
title: Can the content base use the rebate?
status: todo
deps: []
related: [io/camera-scan-input]
summary: "spike: compare --base-content at the default inset and at --measure-inset 0 against measured bases"
---
# Can the content base use the rebate?

## Goal

A spike. The content base (`measure-roll --base-content`) pools each picture frame's
effective area and reads its 99th percentile, so the deepest scene shadows stand in for
the base. The effective area's inset passes over the rebate, the one place on a picture
frame where unexposed film shows. Answer whether including the rebate makes the estimate
better, worse or unstable ([desktop app design](../../design/desktop-app.md) §4).

## What is known

- On the 12 archive rolls the content base lands within -0.069..+0.092 density of each
  roll's unexposed frame (`docs/progress/film-base.md`, `content-fallback`).
- `--measure-inset` already applies to the content base, so the comparison needs no code.
- Risks: sprocket holes and backlight are clearer than the base and would pull the 99th
  percentile up (the existing warnings fire at 0.9 of full scale, or a frame 0.15 density
  clearer than the pool); and the estimate would depend on how much rebate each scan
  shows. The holder is opaque, so it never reaches the 99th percentile.
- The Portra400 rolls are already cropped, so they show no rebate.

## Open questions

1. On rolls that show rebate, does `--measure-inset 0` move the content base toward the
   measured base?
2. Does the answer hold on camera scans (`io/camera-scan-input`)?
3. If the rebate helps, what finds it: a rebate search (a uniform band between holder and
   picture, below the backlight), not a wider area.

## How to Verify

- A table per roll: measured base, content base at the default inset, at inset 0, and
  their density differences, written up under `docs/spike/` or `docs/reports/`.
