---
title: Seed roles for the date-named rolls
status: done
deps: [analysis/asset-manifest]
tags: [post-mvp]
summary: "a frame or roll new to the manifest is seeded from its name: roles from base/leader/calibration stems, stock from the roll name"
---
# Seed roles for the date-named rolls

> **From TASKS.md** (migrated 2026-10-14): done 2026-10-01. A frame or roll new to the manifest is seeded from its name: `base` / `leader` / `calibration` stems give those roles, and stock is parsed from a `<date>-<Stock><speed>` roll name. A from-scratch generation reproduces the live manifest.
>
> **Dependency note** (migrated 2026-10-14): filed 2026-09-24: no `SEED_ROLES` entry matches a date-named roll, so a from-scratch generation marks reference frames `real`

## Goal

Make a from-scratch `nctool manifest generate` keep every roll's reference frames
out of `real`. Roles survive a normal regeneration because they are copied from the
previous manifest; only `SEED_ROLES` (`scripts/analysis/nctool/manifest.py`) covers
the case with no previous manifest, and none of its entries match today's rolls. Most
are keyed by pre-rename roll names and `<date>-<camera>-<serial>` stems, and only
`2026-09-14-Ektar100` has a post-rename seed.

This matters because a frame's `role` is the only thing that keeps a leader or an
unexposed frame out of a measurement. A lost manifest would put `base.tif` and
`leader.tif` into every probe's `real` frames and every `nctool` roll statistic.

## Known

- Since the 2026-09-13 rename, each roll's reference frames are named for their role
  (`base.tif`, `leader.tif`, `calibration.tif`), which suggests a rule by stem rather
  than a seed per roll.
- Some rolls have no reference frame, and a `calibration` frame is its own role.

## Open questions

- A stem rule (`base` → `unexposed`, `leader` → `leader`, …) for every roll, or a seed
  per roll?
- Should the stale pre-rename seeds be deleted, or kept for anyone regenerating from
  an old snapshot?

## How to verify

Generating into an empty directory, with no previous manifest, gives every roll's
reference frames the roles the live manifest has.

## Dependencies

- `analysis/asset-manifest`
