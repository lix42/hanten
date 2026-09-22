# Dependency & Module Hygiene

## Goal

Remove dead weight surfaced by the dependency/module-hygiene review (see
`docs/progress/_unassigned.md`): declared-but-unused crates. Pure cleanup, **no
behavior change, byte-identical output.**

**Re-verified 2026-09-13.** The review's second item, a duplicate `algo::Algorithm`
enum beside `types::Algorithm`, no longer exists: `algo/negative-reconstruction-density-curves`
removed the `Converter` trait, `AlgoParams` and both enums when it introduced the
tagged `reconstruction` object. Only the crate cleanup remains, and its shape changed.

## Scope

`kamadak-exif` and `palette` still have zero references under `src/`. `image` now has
**one** caller, `image::load_from_memory` inside a `#[cfg(test)]` block in
`src/io/ultra_hdr.rs`, so it moves to `dev-dependencies` (or that test decodes through
`tiff` instead) rather than being removed outright.

- Remove the two unused crates and re-home `image`; update the committed `Cargo.lock`.
  `image` pulls a large codec tree, so this trims build time and surface.
- `cargo` does not warn on unused *dependencies*, only unused code, which is why CI
  never caught these. A `cargo-machete` / `cargo-udeps` CI step is a possible
  follow-up, out of scope here.

## Constraints

- **No behavior change.** Output stays byte-identical; nothing on the conversion path
  is touched.
- **CI-clean.** `cargo fmt --all --check` → `cargo clippy --all-targets -- -D warnings`
  → `cargo build` → `cargo test`.
- **Keep the remaining `allow`s justified.** The documented item-level allows that
  cover real API surface stay; do not remove one without a replacement comment.

## How to Verify

- `cargo build --all-targets` and full `cargo test` pass with the crates removed or
  re-homed.
- `grep` confirms zero `kamadak-exif` / `palette` references and no non-test `image::`
  path.
- `Cargo.lock` reflects the dropped crates.
- A `hanten convert` on a sample scan produces output identical to pre-cleanup
  (throwaway `#[ignore]` test; derived numbers only, never sample pixels in context).

## Dependencies

- [Pipeline orchestration](pipeline-orchestration.md) — the dependency removal is
  standalone; the edge records only that the product under cleanup exists.
