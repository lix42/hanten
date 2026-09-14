# Release Readiness

## Goal

Get `nc` ready for a public release: correct the public documentation that
currently misstates the product, choose a license, add crate/release metadata,
define supported platforms, and package binaries. From the release-readiness
review (see `docs/progress/_unassigned.md`).

Two loosely-coupled groups: **(1) documentation-accuracy corrections** — quick,
independent, do-first — and **(2) productization** — license, metadata,
platforms, packaging, some of which need decisions and later-phase work.

## Part 1 — Documentation accuracy (do first; independent)

The docs describe a product that "hasn't started" while the pipeline is fully
built. Correct, at minimum:

- ~~README status~~ and ~~algorithm count~~ — **done** by 2026-09-13 (the README
  `Status` describes the working converter and `docs/TASKS.md` no longer counts
  algorithms; verified by grep).
- **Flag names in docs.** The original item said to replace `--out-depth` with
  `--output-hdr`; that is now backwards. `output/presets` (2026-08-09) removed
  `--output-hdr`/`--output-sdr` and shipped `--out-depth u16|f32`, so
  `pipeline-orchestration.md`'s `--out-depth f32` is correct again. Sweep the
  user-facing docs and the open task files for `--output-hdr` shown as a *live* flag
  (the README quick-start carried one until 2026-09-14); done tasks' files and
  `docs/progress/` legitimately record the old name as history.
- **Research-report citation tokens.** `docs/negative-convertor-research-report.md`
  contains unresolved `citeturn…` tokens throughout. They are **wrapped in
  invisible Unicode private-use characters** (plain `grep citeturn` finds nothing),
  a ChatGPT-export artifact — so cleanup must target the PUA-delimited spans, not
  literal text. Either resolve them to real citations or strip the tokens (and the
  invisible delimiters) so the prose reads cleanly. It's background context, not
  spec, so stripping is acceptable.
- **General sweep.** Grep the docs for other renamed/removed flags and any other
  "planned/not-yet" phrasing that a shipped tool contradicts.

## Part 2 — Productization

### License (DECISION REQUIRED — the user's call)

There is no `LICENSE` file and no `license` field in `Cargo.toml`. Choosing a
license is the user's decision (their IP) — **do not pick one unilaterally.**
Surface the common options (e.g. MIT / Apache-2.0 / dual MIT-OR-Apache-2.0, the
Rust-ecosystem norm) and, once chosen, add the `LICENSE` file(s) and the SPDX
`license` field.

### Crate / release metadata

`Cargo.toml` currently has **none** of the release fields. Add: `description`,
`license` (or `license-file`), `repository`, `readme`, `keywords`, `categories`,
and `authors`/`rust-version` as appropriate. These are required for a polished
release (and mandatory if publishing to crates.io — decide whether that's in
scope).

### Supported platforms

Define and document the target platforms (macOS/Linux/Windows, arch matrix). Note
the real constraint: `nc` links **`lcms2-sys`** (Little CMS C FFI), so each target
needs a working C toolchain / the vendored build to succeed — this shapes what can
be cross-compiled and packaged. State the tested/supported tiers explicitly.

### Packaging binaries

Add a release workflow that builds and publishes binaries per supported platform
(e.g. a tag-triggered GitHub Actions job producing archives + checksums). Sequence
this **after** `display-output-acceptance` so binaries ship only once the default
output is validated — core `real-scan-verification` supplies earlier
resource/pipeline evidence, and the doc fixes in Part 1 need not wait.

## Constraints

- **Accuracy over completeness.** Every corrected doc statement must match the
  actual shipped CLI surface — verify flag names against `cli.rs`, not memory.
- **No behavior change.** This is docs, metadata, and packaging only; the
  conversion pipeline is untouched.
- **License is a user decision** — the task surfaces options and applies the
  choice; it does not make it.
- **Design source.** `design-spec.md` is the sole maintained design document;
  a rendered HTML companion may be regenerated after the feature roadmap
  stabilizes.

## How to Verify

- README no longer claims pre-implementation; usage heading isn't "Planned"; a
  fresh reader would understand the tool works.
- No user-facing doc (README, `docs/using-nc.md`, the design spec, the skills) or open
  task file shows `--output-hdr` / `--output-sdr` as a live flag; done tasks and
  `docs/progress/` history excepted. Flag names match `cli.rs`.
- The research report contains no `citeturn` tokens or their invisible PUA
  delimiters (verify with a PUA-aware scan, not plain-text grep).
- `LICENSE` file present and `Cargo.toml` carries the agreed license + metadata
  fields; `cargo build` still clean.
- (If packaging done) a tagged release produces per-platform binaries + checksums
  for the documented supported platforms.

## Dependencies

- [Pipeline orchestration](pipeline-orchestration.md) — the working product the
  corrected docs describe; Part 1 is executable now.

Sequencing note, not a task dependency: packaging in Part 2 is best done after
[Display-output acceptance](../analysis/display-output-acceptance.md) so binaries ship on
validated defaults. It is not a hard blocker because the documentation corrections
in Part 1 should remain executable now.
