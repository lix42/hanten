# `nctool`'s default-binary test depends on the checkout

## Goal

Make `test_falls_back_to_the_default_binary` (`scripts/analysis/nctool/test_review.py`)
independent of what is built in the checkout, so the `nctool` gate is green whether or
not `target/release/hanten` exists.

## Design

What is known:

- The test calls `review.resolve_binaries([], {}, None)` and expects a `ReviewError`
  naming `review.DEFAULT_NC` (`target/release/hanten`, relative to the working
  directory). It passes only when no such binary exists there.
- Every render review set starts with `cargo build --release` (the `render-review-set`
  skill), so the gate fails in exactly the checkouts that just did review work. CI has no
  release build, so CI stays green and the failure is local only. Found while shipping
  `nf-calibration/anchor-comparison`: the suite passed with the binary moved aside.

Open:

- Whether the test should run from a temporary working directory, point the default at a
  path the test controls, or assert the fallback without depending on the filesystem.

## How to Verify

The `nctool` suite passes both with and without `target/release/hanten` present, and the
test still fails if the fallback to `DEFAULT_NC` is removed.

## Dependencies

None.
