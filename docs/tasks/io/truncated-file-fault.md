---
title: Name a truncated input file
status: todo
deps: []
summary: a file shorter than its own header declares (a cloud file still downloading) gets its own fault, not a decode error
---
# Name a truncated input file

## Goal

A file shorter than its own header declares fails with a fault that says so, instead of
a generic decode error (`advancing to the next IFD: failed to fill whole buffer`, exit 3).
A cloud-synced folder (Google Drive streaming, iCloud) hands the decoder such files
routinely; the desktop app's message handler retries or tells the user the file is not
downloaded yet ([desktop app design](../../design/desktop-app.md) §2.8).

## What is known

- Seen in the prototype spike (`docs/spike/native-app-prototype.md`, unknown 11): 22 of
  36 files on a streamed roll were shorter on disk than the manifest records.
- The app wants a stable code for it (`core/message-codes`).

## Open questions

1. Which exit code and wording; whether the file size vs the header's declared extent is
   checkable before decoding.
2. Whether to wait for a file to become fully available, or only name the fault.

## How to Verify

- A truncated copy of a real scan fails with the named fault in every command that reads
  it (`convert`, `roll`, `inspect`, `measure-*`).
- Complete files decode unchanged.
