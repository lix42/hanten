# Parallel HDR stages

**Done:** 2026-09-16 — see `docs/progress/output.md`.

## Goal

Extend the parallel drivers to the HDR side — `hdr::render`, `hdr::encode_transfer`,
`gain_map::build` and `io::encode::quantize_u16` — with byte-identical output, so
`hdr-linear-tiff`, the coded HDR TIFFs and the gain-map preset stop paying ~500 ms of
sequential per-pixel work ([gpu-rendering-spike](../../gpu-rendering-spike.md),
Experiment 1).

## Design

These loops fuse a per-pixel map with a **reduction**, which is why they are a
separate task: the map is order-free, the reduction is not.

- `hdr::render`: parallel checked map for the pixels; the MaxCLL max and the
  **MaxFALL f64 sum stay a sequential pass** over the rendered buffer, in the
  existing order, so `clli` is bit-identical.
- `hdr::encode_transfer`: parallel checked in-place map.
- `gain_map::build`: parallel checked map producing the P3 HDR and gain triples;
  gain min/max folded afterwards (order-free).
- `quantize_u16`: parallel map plus a parallel integer fold for the clip and
  non-finite counters.
- Reuse the `pipeline::pixels` helper; do not grow it.
- Watch peak memory: the spike's throwaway patch collected into a temporary vector
  and flattened, raising the gain-map preset's RSS from 1.49 to 2.28 GB. Write into
  the destination directly. Re-measure and update `pipeline/memory.rs` if any
  profile's peak buffer set changes.

## How to Verify

- Four gates green; drift gate quiet.
- Byte identity on `hdr-linear-tiff`, `hdr-pq-tiff`, `hdr-hlg-tiff`, `hdr-pq`,
  `hdr-hlg` and `gain-map-hdr` (pre/post `cmp`), including the AVIF `clli` box and
  the reports' `content_light` fields.
- Timing on the spike's frame: `hdr-linear-tiff` colour ~196 → ~70 ms; gain-map
  encode stage ~470 → ~200 ms (the base JPEG remains ~120 ms).
- Peak RSS per preset unchanged within noise.

## Dependencies

- [Parallel display stages](parallel-display-stages.md)
- [Display-HDR rendering](hdr-display-rendering.md)
- [Ultra HDR v1 gain-map JPEG output](gain-map-hdr-output.md)
