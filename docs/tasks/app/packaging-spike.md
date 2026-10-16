---
title: Packaging spike
status: done
deps: [app/webkit-display-spike]
summary: "unsigned Tauri app with the Rust core: hardened runtime, updater, dev keychain fallback, telemetry consent"
done: 2026-10-16
---
# Packaging spike

## Goal

Package the app outside the Mac App Store: a Tauri app with the Rust core inside,
built unsigned or ad-hoc signed, that can update itself
([desktop app design](../../design/desktop-app.md) §2.10, §3.8). Two parts moved out:
signing, notarization and the synced device key, which need Apple Developer Program
enrollment (`app/signed-build-spike`), and file access on real scan folders
(`app/file-access-spike`).

## What is known

- Outside the App Store there is no sandbox requirement; the hardened runtime is needed
  for notarization, and an ad-hoc signature can carry it.
- The core is linked in-process, so no separate `hanten` binary ships.
- The data protection keychain fails with `-34018` from an unsigned binary
  (`docs/spike/offline-token.md`).

## Open questions

1. Does the core, lcms2 included, run under the hardened runtime?
2. Updates for a Tauri app: the updater's own signing, and what an update does to
   keychain items.
3. Telemetry consent in the app.
4. The keychain fallback for unsigned development builds.
5. The signed steps (certificate, provisioning profile, notarization), written out ready
   for `app/signed-build-spike`.

## How to Verify

- An ad-hoc signed build with the hardened runtime that exports in-process and updates
  itself from v1 to v2, recorded in `docs/spike/packaging.md`.
- A development keychain fallback that the release build cannot reach.
- Telemetry consent and the signed steps answered in writing in the same report.
