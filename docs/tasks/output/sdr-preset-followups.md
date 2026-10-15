---
title: SDR Preset Follow-ups (carried-over findings)
status: done
deps: [output/presets]
tags: [post-mvp, no-downstream-blockers]
done: 2026-10-11
---
# SDR Preset Follow-ups (carried-over findings)

> **From TASKS.md** (migrated 2026-10-14): **done 2026-10-11.** Every finding resolved: adding or removing a telemetry enum member is not a schema bump, and a new `destination::Encoding` widens the upload contract with no consent bump (`SCHEMA_VERSION`'s doc, the contract README); the manifest's gamut comes from ICC primaries (#246). Its three design questions became the tasks below
>
> **Dependency note** (migrated 2026-10-14): **done** 2026-10-11

## Goal

Hold the bounded review findings the `display-p3` / `compatibility` preset PRs left
out, so the open space is visible rather than remembered. None blocks anything.

**Rescoped 2026-09-13.** The three design questions this file used to hold are now
their own tasks: [`display-p3-default`](display-p3-default.md),
[`adobe-rgb-gamut`](adobe-rgb-gamut.md) and [`sdr-report-block`](sdr-report-block.md).
What remains here are the carried-over findings below.

## Carried-over review findings

- ~~**The SDR-range warning is luminance-only, so it can misfire on saturated
  colour.**~~ Resolved by `output/content-light-levels` (2026-10-01): MaxCLL is now
  CTA-861.3's per-pixel max(R, G, B), and the warning keeps it as its trigger, so a
  saturated channel above reference white silences it.
- ~~**The telemetry record's preset enum has outrun its schema version.**~~ Settled
  2026-10-11: adding or removing an enum member is not a schema bump (stated in
  `telemetry::SCHEMA_VERSION`'s doc and the `perf-telemetry` skill). The upload's
  closed `conversion.encoding` enum widens with the contract and the Worker in the same
  change, with no consent bump (`contracts/telemetry/upload-v1/README.md`);
  `every_projection_satisfies_the_schema` already fails until it does.
- ~~**The asset manifest infers encoding from bit depth alone.**~~ Resolved by #246
  (2026-10-07): a 16-bit file's gamut comes from its embedded ICC primaries
  (`manifest.py`'s `icc_gamut`), so a Display P3 output is no longer recorded as sRGB.
  Left, deliberately: a coded PQ/HLG TIFF (a LUT profile) records plain `u16`, and an
  nc float output `f32-linear` with no gamut. Nothing reads the field; fix it when
  something does.
- ~~**`nctool compare` does not cover the default preset.**~~ Resolved 2026-10-01 by
  `nf-verification/benchmark-set`: the `fixtures` set has a `default` case with no
  output flag, plus one case per ready destination. A gain-map JPEG's `mean` is still
  the normalized 8-bit buffer handed to the compressor.

## Settled

`RunProfile::SdrTiff` is calibrated (2026-08-09): peak RSS 0.850 GB at 15.55 MP and
3.594 GB at 74.65 MP against estimates of 0.921 / 3.911 GB, `accounted` under measured
as the allowance requires; two frame sizes per the calibration rule. The table in
`pipeline::memory`'s module doc carries the rows.

## How to Verify

Each finding above is either fixed, with a test, or handed to a named owner task, and
this file says which. When the list is empty the task closes.

## Dependencies

- [Output presets and guidance](presets.md)
