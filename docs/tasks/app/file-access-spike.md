---
title: File access spike
status: todo
deps: [app/packaging-spike]
summary: "the packaged app on scan folders: external drive, Documents/Desktop, iCloud Drive, a read-only folder; do access grants survive a relaunch and an update"
---
# File access spike

## Goal

Confirm the packaged app can read a roll and write its project file wherever the user
keeps scans ([desktop app design](../../design/desktop-app.md) §2.8, §3.8), using the
unsigned spike app from `app/packaging-spike`.

## What is known

- The spike app (`docs/spike/packaging.md`) is ad-hoc signed with the hardened runtime
  and no sandbox. It lists and probes a picked folder, writes `roll.hanten.json` beside
  the scans, and measures and exports in-process. All of that ran on a local folder,
  none of it on a protected location.
- Its code is a portable copy described in the report's "How to rebuild". The report also
  has the step-by-step checklist.
- Outside the sandbox the app reads whatever its user can. What remains is macOS's
  privacy prompts (TCC), and an ad-hoc signature changes with every build.

## Open questions

1. Does a folder on an external drive, in `~/Documents` or `~/Desktop`, or in iCloud
   Drive prompt when picked, and is it readable and writable after?
2. Does access hold after a relaunch, and after an ad-hoc update, without the dialog?
3. A cloud scan that isn't downloaded yet: how long does reading it take, and what fault
   comes back when the download fails (`io/truncated-file-fault`)?
4. Does a read-only folder give a named write fault the app can act on?
5. The export time for a real roll on the external drive.

## How to Verify

- The checklist in `docs/spike/packaging.md` run with a real roll on the external drive,
  with results in that report (or a note beside it). Anything that only a signed build
  can answer goes to `app/signed-build-spike`.
