# Check the uploader against the live endpoint and on macOS

## Goal

Confirm on a real Mac that `hanten telemetry` works end to end against the
production Worker, which `telemetry/upload` could not reach: that session's proxy
refused the Worker's host, and it had no macOS machine (CI's macOS job runs the
unit and loopback tests only).

## What to check

- The default build's endpoint is the live Worker, and `hanten telemetry enable`
  then a `convert` lands one event: `flush` reports it accepted (or `duplicate` on a
  second flush), and D1 holds it — in quarantine until the release is listed in
  `allowed_releases` (`services/telemetry-ingest/README.md`). Decide whether to list
  the release now.
- TLS through rustls' bundled roots works from a Mac, with and without an
  `HTTPS_PROXY`.
- macOS behaviour the Linux run cannot show: where the consent record lands
  (`$XDG_CONFIG_HOME/nc`, else `~/.config/nc` — not `~/Library`; decide whether that
  is right for a Mac), `fsync` and rename durability on APFS, and that the detached
  helper survives the terminal closing and Ctrl-C.
- `hanten telemetry status` after a run shows `last_success_ms` and no error.

## Dependencies

- [Background telemetry upload](upload.md)
