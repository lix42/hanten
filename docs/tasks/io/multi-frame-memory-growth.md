# Multi-frame runs outgrow the per-frame memory model

## Goal

Make a multi-frame run — `roll`, `measure-roll` — stay within what the memory gate
approved, or make the gate account for it. Today the gate judges each frame alone,
and a whole roll can peak several times higher with nothing warning.

## Known

- **Measured on Gold200 (macOS/aarch64, peak RSS):** one frame of `measure-roll`
  0.61 GB, 35 frames 2.3–3.4 GB; `roll` 0.70 GB for one frame, 2.70 GB for 35.
  (`src/pipeline/memory.rs`'s calibration notes; `docs/progress/nf-scene-correction.md`,
  `roll-white-balance`.)
- **The cause is frame sizes, not a leak.** The same frame twice stays flat; a smaller
  frame then a larger one grows, the reverse does not. macOS malloc keeps freed large
  blocks resident for reuse (`vmmap`: `MALLOC_LARGE (empty)`), and a block cannot serve
  the next, slightly larger frame. `malloc_zone_pressure_relief` releases none of it.
- The per-frame model itself is calibrated and sound for one frame; what it could not
  see is the run.

## Answered (2026-10-01)

- **Remedy in the allocation or in the gate?** The allocation: a global allocator
  (`src/allocator.rs`) maps blocks of 8 MiB or more directly and unmaps them on free.
  The gate is unchanged; a run now peaks at its largest frame. Buffer reuse across
  frames and a roll-level growth term were the alternatives, not taken.
- **Does Linux behave the same?** glibc already maps blocks over its mmap threshold
  directly, so the growth was macOS's; `tests/multi_frame_memory.rs` checks both CI
  platforms.
- **`io/streaming-tiled-io`:** fixed-size tiles would not have hit this, and tiles
  under 8 MiB go through malloc, which reuses same-size blocks.

## How to Verify

- A multi-frame run over one real roll peaks within the gate's estimate, or the gate
  refuses it before decoding; measured on both platforms CI runs.
- A single-frame run's peak and output are unchanged.

## Dependencies

- [Memory preflight & in-place transform](memory-preflight.md) — **done**; the
  per-frame model this extends.
