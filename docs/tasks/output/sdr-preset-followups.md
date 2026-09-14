# SDR Preset Follow-ups (carried-over findings)

## Goal

Hold the bounded review findings the `display-p3` / `compatibility` preset PRs left
out, so the open space is visible rather than remembered. None blocks anything.

**Rescoped 2026-09-13.** The three design questions this file used to hold are now
their own tasks: [`display-p3-default`](display-p3-default.md),
[`adobe-rgb-gamut`](adobe-rgb-gamut.md) and [`sdr-report-block`](sdr-report-block.md).
What remains here are the carried-over findings below.

## Carried-over review findings

- **The SDR-range warning is luminance-only, so it can misfire on saturated colour**
  (`pipeline/hdr.rs`, `sdr_range_warning`). MaxCLL is a luminance measure, so a
  rendered BT.2020 blue near `[0, 0, 4]` sits around 48 nits of luminance while its
  blue channel uses substantial per-channel headroom that no SDR-range signal can
  carry without clipping or shifting the colour. The warning would then claim the
  whole signal is SDR-range and `--strict` would fail valid HDR colour-volume content.
  Fix by also checking the rendered per-channel peak, or by narrowing the claim to
  "no *luminance* headroom"; the latter is the smaller change and matches what MaxCLL
  witnesses.
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
- **`nctool compare` does not cover the default preset.** `benchmark.json`'s fixture
  cases state `--output-preset legacy` explicitly so they stay comparable with records
  made before the default flip, so the product default is not in the fixed comparison
  set at all and a cross-build comparison says nothing about the container users get.
  Adding a case changes that fixed set, which was `core/conversion-versioning`'s call;
  that task is closed, so this needs an owner. The units question rides along: `mean`
  for a JPEG preset is the normalized 8-bit buffer handed to the compressor.

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
