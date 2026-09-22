# Parallel display stages

**Done:** 2026-09-16 — see `docs/progress/output.md`.

## Goal

Run every per-pixel stage between reconstruction and the u16 encode on all cores,
with **byte-identical output**. Today only reconstruction uses rayon; the lcms2
transform, the SDR render, the ACEScg mapping and the print controls are
sequential and cost ~1.1–1.3 s of a 1.4–1.6 s `legacy`/`display-p3` run on a
16.4 MP scan ([gpu-rendering-spike](../../spike/gpu-rendering-spike.md), Experiment 1).

## Design

Follow the spike's "Architecture for the multithreading work": no layer over
rayon, no executor trait, a kernel/driver split only where a loop fuses a map
with a reduction, and one small helper.

- A `pipeline::pixels` helper: an in-place map over RGB triples, a checked map
  that returns the first error with its pixel index, and an integer fold. One
  place for the `as_chunks` guard; the API cannot express a
  parallel f32 sum. Nothing else goes in it.
- `color::transform_in_place`: build the lcms2 transform with `Flags::NO_CACHE`
  (lcms2's own condition for sharing a transform across threads — the crate marks
  only its `DisallowCache` type `Sync`, which the generic `new_flags_context`
  constructor returns; the `GlobalContext` shortcuts `new`/`new_flags` erase the
  flag from the type) and transform row chunks in parallel.
- `sdr::render`: parallel checked map of `render_pixel_checked`.
- `working_space::map_nc_film_rgb_v1`, `render_split::apply_shared_controls`:
  parallel in-place maps.
- No new full-frame buffer: write in place or collect straight into the output.
  `pipeline/memory.rs` is untouched if that holds; otherwise update it.

Open: the chunk size (rows vs fixed pixel counts) — measure, don't guess.

## How to Verify

- All four gates green (`cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo build`, `cargo test`); the drift gate must stay quiet (no fingerprint moves).
- Byte identity on real scans: render `legacy`, `display-p3`, `film-master`,
  `hdr-linear-tiff` and `gain-map-hdr` from the 2026-09-13 Portra 400 roll's `1675.tif`
  with the pre-change and post-change binaries and `cmp` the files. The spike's
  Reproduction section has the command line.
- Timing: `--telemetry` colour stage on that frame drops from ~1100–1300 ms to
  ~110–250 ms; peak RSS (`/usr/bin/time -l`) does not rise.

## Dependencies

- [SDR display rendering](sdr-display-rendering.md)
- [Film-master render pipeline](../color/film-master-render-pipeline.md)
