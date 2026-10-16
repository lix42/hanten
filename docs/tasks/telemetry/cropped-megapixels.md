---
title: Count a cropped frame's megapixels in telemetry
status: todo
deps: [core/crop-rotate]
priority: low
summary: Telemetry's megapixels is the decoded scan's size; after a --crop every stage past the decode runs on fewer pixels, so per-MP stage timings understate cropped runs.
---
# Count a cropped frame's megapixels in telemetry

## Goal

Telemetry's megapixels is the decoded scan's size; after a --crop every stage past the decode runs on fewer pixels, so per-MP stage timings understate cropped runs.

## Design

Known: `telemetry.rs` records `megapixels` from the decoded scan, and the decode
and film-base stages do run on the whole scan. Unknown: whether to add a second size
(the cropped frame) or record the crop, and what that does to the upload contract
(`contracts/telemetry/upload-v1/README.md`) and to the server's scale buckets.

## How to Verify

A `convert --crop` run's telemetry event states the size the post-decode stages ran
on, the upload manifest lists the field, and an uncropped run's event is unchanged.
