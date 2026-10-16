---
title: Preview cache and memory spike
status: todo
deps: []
summary: "a whole roll: cold load, one decode shared by measuring and preview, a memory budget, a disk cache"
---
# Preview cache and memory spike

## Goal

Measure the core over a whole roll, not one frame, to design the preview cache
([desktop app design](../../design/desktop-app.md) §3.5).

## What is known

- From the prototype: the cache point is the downsampled scan; a 2048-edge scan is 22 MB,
  so a 36-frame roll holds about 0.8 GB; each prepare briefly holds a full decode
  (~260 MB at 16 MP); a cold read from a cloud folder is ~750 ms per 134 MB frame.
- Measuring ran as a separate `measure-roll` process, so every frame decoded twice
  (spike unknown 6); the design measures in the core.
- `pipeline/memory.rs` models one run, not a long-lived cache (spike unknown 7).
- From `app/export-beside-preview-spike` (`docs/spike/export-beside-preview.md`):
  - Export runs one frame at a time beside the cache, adding ~300 MB. Export decodes
    again, re-reading a file the preview already read, so the page cache (or a cold
    cloud read) matters twice.
  - Preparing a frame while an export runs was not measured.
  - Background work in the core (measuring, filling a disk cache) can't use a lower QoS:
    on Apple silicon it slowed the export 7× or more.

## Open questions

1. Time to first preview and to a fully measured roll, warm and cold.
2. One decode shared by measuring and preview: what it saves.
3. A memory budget and what is evicted first.
4. Whether a disk cache of downsampled scans makes reopening a roll quick enough to keep.

## How to Verify

- A report under `docs/spike/` with the timings and peak memory on a real 36-frame roll,
  and a recommended cache design.
