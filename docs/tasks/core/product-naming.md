# Name the product Hanten, and fix the boundary

## Goal

Give the product its own name — **Hanten** — while `nc` stays the internal name of the
crate and of the identifiers, and **write the boundary down** so it is not
re-litigated every time someone notices the two names disagree.

## Design

**The boundary lives in CLAUDE.md**, in the "Hanten outside, `nc` inside" section
under "What this project is". It is the authoritative list of what is branding and
what is an identifier; this file deliberately does not restate it.

## Decisions taken

- **The binary *is* `hanten`.** The task originally leaned the other way, but nc is
  unreleased, so the window where this is free closes at the first release. Done with
  a Cargo `[[bin]]` section, which leaves the package — and therefore `NC_VERSION`
  and the `nc_version` report field — untouched.
- **`--help` and `--version` name the product.** `about` reads "Hanten — …"; the
  `--version` banner gets the name from the binary, so `version_string()` was left
  alone (prefixing it too prints `hanten Hanten 0.1.0`).
- **Diagnostics are prefixed `hanten:`.** Nothing parses the prefix —
  `scripts/real-scan-verify/harness.sh` greps the message text.
- **The repository is `lix42/hanten`**, private. The linked worktrees share one
  config, so `origin` was repointed once rather than per worktree.
- **History keeps the old spelling.** Command lines in `docs/progress/`,
  `docs/reports/`, `docs/spike/` and closed task files record what was *run*. Two
  exceptions: `cargo --bin nc` is build machinery that would simply fail, and it was
  fixed everywhere.

## How to Verify

- `grep -rn "Hanten"` finds it only in branding, CLAUDE.md's boundary, and
  user-facing strings; **no identifier acquired it**. Grepping the old product name
  returns only the historical research report (and this file's own mention of it).
- All six CI gates pass, and the report's `working_mapping` still reads
  `nc-film-rgb-v1` with the telemetry schema unmodified.
- `nctool` still recognises a pre-rename binary by its `--version` banner, which the
  reference-rendition workflow depends on
  (`nctool.test_manifest.TestBinaryResolution`).

## Dependencies

None.
