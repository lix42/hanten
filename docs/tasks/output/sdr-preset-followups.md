# SDR Preset Follow-ups (carried-over findings)

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
- **The telemetry record's preset enum has outrun its schema version.** `OutputPreset`
  has twelve variants, ten added after the record last bumped for a preset reason. Is
  *adding* an enum member a wire-shape change (the module's rustdoc rule, read
  strictly) or an additive one a forward-compatible consumer tolerates?
  `telemetry/ingestion-service` is the consumer that makes it matter. The v4 bump on
  2026-08-09 was for the `output_hdr` → `output_depth` field rename and does not answer
  this.
- **The asset manifest infers encoding from bit depth alone**
  (`scripts/analysis/nctool/manifest.py`, the `bits == 16` arm). Every 16-bit `nc`
  output is labelled `u16-srgb`, true only while `nc` had no other 16-bit SDR encoding.
  A `display-p3` result dropped into `converted/nc/` would be recorded as sRGB and any
  cross-encoding analysis would decode it with the wrong primaries. Latent today; the
  fix needs real provenance (the sidecar's `output_render.encoding`), not a second
  filename guess.
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
