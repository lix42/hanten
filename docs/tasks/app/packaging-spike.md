---
title: Packaging and file access spike
status: todo
deps: []
summary: sign and notarize a Tauri app with the Rust core; write the project file into any user-picked folder
---
# Packaging and file access spike

## Goal

Ship the app outside the Mac App Store: sign and notarize a Tauri app with the Rust core
inside, and confirm it can write the project file into any folder the user picks
([desktop app design](../../design/desktop-app.md) §2.8, §2.10).

## What is known

- Outside the App Store there is no sandbox requirement; the hardened runtime is needed
  for notarization.
- The core is linked in-process, so no separate `hanten` binary ships.

## Open questions

1. Signing, notarization and updates for a Tauri app.
2. File access under the hardened runtime: scan folders, external drives, cloud folders.
3. Telemetry consent in the app.

## How to Verify

- A notarized build that opens a roll on an external drive, writes its project file and
  exports, recorded under `docs/spike/`.
