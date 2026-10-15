---
title: Extract the chart patches
status: done
deps: [analysis/chart-labeling-tool]
tags: [post-mvp]
summary: one per-patch table from the scans, read by every calibration consumer
done: "2026-10-14 #296"
---
# Extract the chart patches

> **Dependency note** (migrated 2026-10-14): filed 2026-10-14 (split from `calibration-frame-capture`): one per-patch table from the scans that every calibration consumer reads

## Goal

One extraction that reads every labelled chart and writes a per-patch table — roll, frame,
target, patch, the scan's value per channel and its spread, with the frame's shot facts and
the roll's conditions — that every calibration consumer reads, instead of each consumer
re-reading the frames its own way.

## Known

- Agreed with the user 2026-10-14: one extraction, many consumers. The consumers are
  `calibration-frame-capture`'s.
- Reads the scan, not a render: a bracket's over end is clipped by any render (on
  `2026-10-12-Gold200` +3/+4 lose 6% and 21% under `--placement frame`).
- Each frame carries two neutral series: the Classic's six and the Creative target's
  column of eight (four near-white, four near-black; Calibrite publishes no values).
- Patch positions are projected from the chart entries at read time; nothing per patch is
  stored in the manifest.
- Never reads pixels into an agent's context; asset-gated like every real-scan check.

## Decided (user, 2026-10-14)

- Both units: median transmission (the decoder's scale) and density relative to the
  roll's film base, measured by `hanten measure-base` on its unexposed frame; a p10-p90
  density spread and pixel count per patch and channel. Decoded or rendered values stay
  with the consumers that want them; no consumer asks for producers' outputs (NLP,
  SilverFast, CCR, Hanten), so they are out of scope.
- The table lives beside the manifest as `charts.json`, rebuilt by
  `nctool manifest charts table`; each frame carries its shot facts (aperture included)
  and each roll its stock, conditions and film base.

## How to Verify

- The table covers every labelled chart on the archive and reruns byte-identical.
- The neutral series rise monotonically with exposure across the 2026-10-12-Gold200 bracket.

## Dependencies

- [Label the charts in a frame](chart-labeling-tool.md) — the chart entries it reads
