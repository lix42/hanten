# Make `display-p3` the default output preset

> **Superseded 2026-09-19 by `nf-destinations/default-destination`** — the
> destination set is redesigned and the default moves with the new flow; the
> product shape and the one-bump argument carried over. Kept so existing
> references resolve; see `docs/nf-migration.md` for the migration plan.

## Goal

Execute the 2026-08-09 decision that the default output becomes `display-p3`, a
16-bit SDR TIFF, replacing the incumbent `gain-map-hdr` JPEG. The choice is made
(wide-gamut fidelity outweighs sRGB's "surprise nobody" argument); this task is the
execution.

## Direction decided; order open

Split out of `output/sdr-preset-followups` on 2026-09-13 because it collides with
`algo/split-default-migration`: that task changes the default *reconstruction* and one
of its stated payoffs is un-inerting the default gain map (MaxCLL 101 → 999 nits under
the characteristic curve), while this one changes the default *container* to one with
no gain map at all.

**Decided 2026-08-09 and reaffirmed 2026-09-13 (user): SDR lossless is the default.** The product shape around
it: HDR lossless stays supported (`hdr-linear-tiff`, `hdr-pq-tiff`, `hdr-hlg-tiff`);
SDR JPEG is to be supported (`output/sdr-jpeg-preset`, filed the same day); HDR JPEG
(`gain-map-hdr`) is good to have and stays opt-in. The migration's HDR payoff therefore
reaches the explicit gain-map presets, not the default path.

Still open is the **order**. Both tasks owe a `pipeline_version` bump and a
before/after report; landing them separately is two bumps and two migrations for
users, and every recipe silent on `output.preset` changes container at the second.
Preferred: one bump carrying both, with one report. Decide before either lands.

## What is known

- A **pixel change** (a different pipeline, not just a different profile) plus a
  container change, so it needs its own `pipeline_version` row, a before/after report
  like `reports/render-defaults-v3.md`, and broad test churn: every test exercising the
  default output path, and `tests/pipeline.rs`'s `run()` helper, which injects
  `--output-preset legacy` for the ~87 tests that predate the gain-map default.
- `legacy` and its frozen `stages::golden` vectors become deletable once a modern-path
  preset is the default. Do not delete them first.
- `default_output_name` and `cli::derived_extension` already derive the suffix from the
  resolved preset, so `hanten roll` follows the flip for free.

## How to Verify

- `display-p3` resolves with no output-selection options; the before/after report
  exists and shows what changed on real frames; `pipeline_version` bumped with a
  recorded row; `legacy` either deleted or explicitly retained with a reason.
- `docs/using-nc.md` updated by running the binary.

## Dependencies

- [Output presets and guidance](presets.md)

Coordinate with [`algo/split-default-migration`](../algo/split-default-migration.md);
deliberately not an edge until the order is decided.
