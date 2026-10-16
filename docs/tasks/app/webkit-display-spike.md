---
title: WebKit display spike
status: done
deps: []
summary: Display P3 canvas, HDR preview and slider feel in WKWebView; decides Tauri vs Electron
done: 2026-10-15
---
# WebKit display spike

## Goal

Answer whether WKWebView can show what the app needs, before any UI is built on Tauri
([desktop app design](../../design/desktop-app.md) §3.1). If it can't, fall back to Electron, then Dioxus or GPUI.

## What is known

- The prototype (`docs/spike/native-app-prototype.md`) rendered previews in-process and
  sent RGBA to a canvas, but never ran the interactive checks: whether the canvas
  honours `colorSpace: "display-p3"`, the slider-to-frame round trip, and how
  newest-request-wins feels during a fast drag.
- The prototype rendered a knob change in 24–35 ms at a 2048 edge (this spike measured
  ~73 ms, 37–54 ms without lcms2); the canvas scales to the window.
- HDR canvas support differs between WebKit and Chromium; nothing is verified.

## Open questions

1. Does a Display P3 canvas in WKWebView show P3 colours unclipped?
2. Can an HDR preview be shown at all: an HDR canvas, an image element fed a gain-map or
   PQ file, or nothing?
3. The round trip from slider to drawn frame, against the render time.
4. How the canvas compares with the exported file, viewed side by side.

## How to Verify

- A short report under `docs/spike/`: each question answered on this Mac, with the test
  page, and a stay-or-switch verdict for Tauri.
