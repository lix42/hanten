---
title: Offline token spike
status: done
deps: []
summary: how tokens are signed, verified, stored and recovered on macOS
done: 2026-10-15
---
# Offline token spike

## Goal

Choose how payment tokens work on the device ([desktop app design](../../design/desktop-app.md) §2.11, §3.7): bought online, spent
offline, checked in the core.

## What is known

- Light protection is enough, with no performance cost, using the system's protection
  where possible.
- The paid record lives in the project file: per-frame content hashes plus the token's
  signature. Losing the file means paying again.
- The signing key stays on the server; the core holds the public key.

## Open questions

1. Token format and signature scheme.
2. Where unspent tokens live on macOS (Keychain?) and how a reinstall recovers them.
3. What the token website must provide (accounts, licence keys, a recovery path).
4. How the replacement allowance (a limited number of rescanned frames) is recorded.
5. How a token is bound to one roll. The server signs before the roll's hashes exist,
   so the signature cannot cover them: a paid record copied to another project or
   hand-edited must still fail the check (a spent-token ledger outside the project
   file, a sealed record).

## How to Verify

- A short design note under `docs/spike/` and a throwaway sign/verify round trip.
