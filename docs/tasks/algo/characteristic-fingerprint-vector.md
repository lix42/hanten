# A portable fingerprint vector for the characteristic curve

## Goal

Produce a `PIPELINE_FINGERPRINTS` `render` vector whose pixels are bit-identical on
both CI targets under the `characteristic` curve, so `algo/split-default-migration` can
write its version row without discovering the portability problem on the day it flips
the default.

## Why it is its own task

Split out of `split-default-migration` on 2026-09-13. `characteristic-curve-coverage`
established by observation that **x86_64 and macOS return different `f32` results from
`log10f`** on two of the fifteen `stages::golden::pixels()` samples under this curve.
The golden there survives with a derived per-sample window
(`stages::golden::reachable_window`); a fingerprint row has **no window at all**, it
hashes raw f32 bits, so the current vector is known to fail it. This is the hard,
frame-independent part of the migration, and it can run now while the migration waits
on the calibration frames.

## What is known

- The chain has two libm calls (`log10` in `to_density`, `10^` in the curve), and a
  1-ULP difference in the first is amplified by `ln(10)·d·(1/γ_local)`, up to 62 pixel
  ULPs. Two threshold-based arguments for "safe" samples were tried during
  `characteristic-curve-coverage` and both were unsound; the margin test it committed
  records which four samples are not safe.
- `golden::pixels()` is shared with every historical row, so moving it shifts their
  meaning. A separate vector for the new fingerprint is likely cheaper.
- Verification is by running CI on both targets, never by a margin argument.

## Open questions

1. Choose sample values by construction (densities whose `log10` is exactly
   representable, or far from an f32 rounding boundary on both libms) or by search?
2. Does the vector also need to cover the reinhard display operator, or does the
   `render` hash stop at reconstruction as today?

## How to Verify

- A candidate vector renders bit-identically on macOS/aarch64 and x86_64 Linux CI,
  proven by a test that hashes it on both, before any `PIPELINE_FINGERPRINTS` row uses
  it.
- Historical rows and `golden::pixels()` are untouched.

## Dependencies

- [Pin the characteristic curve against regression](characteristic-curve-coverage.md)
  — the margin harness and the observed non-portable samples.
