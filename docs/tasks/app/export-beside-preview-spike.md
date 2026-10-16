---
title: Export beside preview spike
status: todo
deps: []
related: [app/preview-cache-spike]
summary: "an in-process export queue while sliders move: does the preview stay smooth, how do they share CPU and memory"
---
# Export beside preview spike

## Goal

Check that an in-process export queue can run while the user drags sliders, without the
preview stuttering ([desktop app design](../../design/desktop-app.md) §3.6).

## What is known

- Full-resolution `convert` took 0.33 s per frame in the prototype (warm cache); a
  preview render at a 2048 edge is 37–54 ms with the display curve applied directly, ~73 ms
  through lcms2 (`docs/spike/webkit-display.md`).
- The design runs export in the core process, not as a spawned binary, and gives the
  preview priority.
- Memory: a full-resolution export holds several full-frame buffers
  (`pipeline/memory.rs`), on top of the preview cache.

## Open questions

1. Preview latency while a roll exports, with and without a priority scheme.
2. How many exports run at once, given memory and cores.
3. Whether export reuses the preview's decode or decodes again.
4. How an lcms2 fault stays with the render that raised it: today's handler is one
   process-global flag, cleared after each render, so a preview and an export can swallow
   or take each other's fault (design §2.7).

## How to Verify

- A report under `docs/spike/` with preview latency during export and peak memory, and a
  recommended scheduling.
