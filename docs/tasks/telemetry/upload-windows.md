# Run the uploader's tests on Windows

**Low priority** (user, 2026-10-01).

## Goal

`telemetry/upload` was compile-checked for Windows only. Run its behaviour there:
the strategy asks for Windows subprocess tests of locking, the detached helper and
the purge race, and for one lock domain with no split-brain lock recreation.

## Open questions

- Add a `windows-latest` CI job, or check by hand? The crate's native
  dependencies (libaom, lcms2) must build there first.
- `durable::sync_dir` is a no-op on Windows and the no-follow checks are a
  `symlink_metadata` pre-check rather than `O_NOFOLLOW`: is that enough?
- Does `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` keep the helper alive after
  the console closes?

## Dependencies

- [Background telemetry upload](upload.md)
