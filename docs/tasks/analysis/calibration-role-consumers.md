# Teach the role consumers the `calibration` role

## Goal

Make every reader of a frame's `role` keep a `calibration` frame out of `real`, as the
`asset-manifest` skill promises for any non-`real` role.

## Known

- The live manifest has one: `2026-09-11-Portra400`'s `calibration.tif`, a half-leader /
  half-base frame. A from-scratch `manifest generate` seeds it too
  (`analysis/manifest-seed-roles`).
- `nctool manifest roles` warns on it and treats it as `real`, so the harness converts it.
- `nctool roll` refuses the whole roll ("unsupported role").
- `src/pipeline/shadow_metrics.rs` counts it as `real` with no warning.
- `src/pipeline/branch_probe.rs` already filters on `role == "real"` and is correct.
- `analysis/calibration-frame-capture` will add a role for bracketed target frames, so
  the fix should not hard-code a fixed list of non-`real` roles where filtering on
  `real` would do.

## Open questions

- Should `manifest roles` and `nctool roll` accept any role and keep only `real`, or
  keep refusing an unknown spelling (a typo'd `leadr` should still be loud)?

## How to verify

`manifest roles`, `nctool roll` and `shadow_metrics` all leave `calibration.tif` out of
`2026-09-11-Portra400`'s real frames, and a typo'd role is still reported.

## Dependencies

- `analysis/asset-manifest`
