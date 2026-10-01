# Check the uploader against the live endpoint and on macOS

## Goal

Confirm on a real Mac that `hanten telemetry` works end to end against the
production Worker, which `telemetry/upload` could not reach: that session's proxy
refused the Worker's host, and it had no macOS machine (CI's macOS job runs the
unit and loopback tests only).

## What to check

- The default build's endpoint is the live Worker, and `hanten telemetry enable`
  then a `convert` lands one event: `flush` reports it accepted (or `duplicate` on a
  second flush), and D1 holds it — in `events` if the release is in
  `allowed_releases`, else in quarantine (`services/telemetry-ingest/README.md`).
- TLS through rustls' bundled roots works from a Mac, with and without an
  `HTTPS_PROXY`.
- macOS behaviour the Linux run cannot show: where the consent record lands
  (`$XDG_CONFIG_HOME/nc`, else `~/.config/nc` — not `~/Library`; decide whether that
  is right for a Mac), `fsync` and rename durability on APFS, and that the detached
  helper survives the terminal closing and Ctrl-C.
- `hanten telemetry status` after a run shows `last_success_ms`, and no error newer
  than it.

## Decisions (user, 2026-10-01)

- `0.1.0` stays in `allowed_releases`, where the first migration put it (the task
  assumed it was not listed): dev builds are analysed until the version moves.
- The consent record stays at `~/.config/nc` on macOS, beside the XDG queue.
- `status` keeps the last error as history after a later success.
- The check's test events are deleted from D1 afterwards.

## Dependencies

- [Background telemetry upload](upload.md)
