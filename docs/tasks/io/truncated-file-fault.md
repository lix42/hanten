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
A cloud-synced folder (Google Drive streaming, iCloud) can hand the decoder such files
for most of a roll; the desktop app's message handler retries or tells the user the file is not
downloaded yet ([desktop app design](../../design/desktop-app.md) §2.8).

## What is known

- Seen in the prototype spike (`docs/spike/native-app-prototype.md`, unknown 11): 22 of
  36 files on a streamed roll were shorter on disk than the manifest records.
- The app wants a stable code for it (`core/message-codes`).
- iCloud Drive is a second case: a file whose data is only in the cloud (`SF_DATALESS`)
  is downloaded whole by its first read. Offline, that read fails with `ETIMEDOUT`
  (errno 60), which today surfaces as exit 3, `not a readable TIFF: Operation timed out`
  (`docs/spike/file-access.md`).

## Open questions

1. Which exit code and wording; whether the file size vs the header's declared extent is
   checkable before decoding.
2. Whether to wait for a file to become fully available, or only name the fault.

## How to Verify

- A truncated copy of a real scan fails with the named fault in every command that reads
  it (`convert`, `roll`, `inspect`, `measure-*`).
- Complete files decode unchanged.
