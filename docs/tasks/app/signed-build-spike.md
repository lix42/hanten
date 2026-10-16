---
title: Signed build spike
status: todo
deps: [app/packaging-spike]
related: [app/file-access-spike]
priority: blocked
summary: notarize the Tauri app and run a synced P-256 key in the data protection keychain across two Macs; waits on Apple Developer Program enrollment
---
# Signed build spike

## Goal

Finish what `app/packaging-spike` could not run unsigned: a notarized Tauri app with the
Rust core inside, and the device key the payment proposal needs
([desktop app design](../../design/desktop-app.md) §2.12, §3.7, §3.8;
`docs/spike/offline-token.md`).

## What is known

- Blocked on Apple Developer Program enrollment: a Developer ID Application certificate,
  notary credentials, and a provisioning profile that authorizes the keychain
  entitlements.
- The unsigned shell, the steps written out ready to run, and what an ad-hoc build
  showed are in `docs/spike/packaging.md` (from `app/packaging-spike`).
- The data protection keychain needs entitlements a provisioning profile authorizes
  (TN3137); an unsigned binary gets `-34018`.

## Open questions

1. Does notarization accept the bundle, and do the keychain entitlements in the report
   give the data protection keychain?
2. Does a synchronizable P-256 key item sync through iCloud Keychain, and survive an
   update?
3. Do keychain access and a prompt-based folder grant (a Files & Folders toggle) survive
   a signed update, unlike an ad-hoc one? A folder picked in the open panel already
   survives an ad-hoc update (`docs/spike/file-access.md`).

## How to Verify

- A notarized, stapled build that Gatekeeper accepts on a Mac that never built it,
  recorded in `docs/spike/packaging.md`.
- The same build creates, signs with and reads back a synchronizable P-256 key in the
  data protection keychain; the key reaches a second Mac, and survives an update.
