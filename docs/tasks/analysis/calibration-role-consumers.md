---
title: Teach the role consumers the `calibration` role
status: done
deps: [analysis/asset-manifest]
tags: [post-mvp]
summary: ": `manifest roles`, `nctool roll`, `viewer` and `shadow_metrics` leave a `calibration` frame out of `real`"
done: 2026-10-10
---
# Teach the role consumers the `calibration` role

> **From TASKS.md** (migrated 2026-10-14): **done 2026-10-10**: `manifest roles`, `nctool roll`, `viewer` and `shadow_metrics` leave a `calibration` frame out of `real`; the known roles are `nctool.manifest.ROLES`, and `validate` flags any other spelling
>
> **Dependency note** (migrated 2026-10-14): filed 2026-10-01: `manifest roles` and `shadow_metrics` count a `calibration` frame as `real`, and `nctool roll` refuses its roll

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
