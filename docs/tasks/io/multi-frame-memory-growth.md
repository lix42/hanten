# Multi-frame runs outgrow the per-frame memory model

## Goal

Make a multi-frame run — `roll`, `measure-roll` — stay within what the memory gate
approved, or make the gate account for it. Today the gate judges each frame alone,
and a whole roll can peak several times higher with nothing warning.

## Known

- **Measured on Gold200 (macOS/aarch64, peak RSS):** one frame of `measure-roll`
  0.61 GB, 35 frames 2.3–2.8 GB. `roll --new-flow` grows the same way: 0.70 GB for one
  frame, 1.30 GB for five. (`src/pipeline/memory.rs`'s calibration notes;
  `docs/progress/nf-scene-correction.md`, `roll-white-balance`.)
- **The cause is frame sizes, not a leak.** The same frame five times stays flat
  (0.62 GB). Frames of one roll differ by a few pixels (4894–4948 wide), and the
  allocator cannot reuse a freed full-frame buffer for a slightly larger one, so each
  new size adds resident pages.
- The per-frame model itself is calibrated and sound for one frame; what it cannot see
  is the run.

## Open questions

- **Remedy in the allocation or in the gate?** Reusing the run's buffers across
  frames, sized once for the largest frame, would bound the growth where it arises. A
  roll-level preflight (the largest frame's peak plus a measured growth term) would at
  least make the gate honest. Returning freed pages to the OS is platform-dependent.
- **Does Linux behave the same?** Only macOS was measured; CI and many users are on
  Linux, whose allocator returns large blocks differently.
- **How it relates to `io/streaming-tiled-io`**, which would shrink every per-frame
  buffer and may make this moot — or not, if its tiles vary in size the same way.

## How to Verify

- A multi-frame run over one real roll peaks within the gate's estimate, or the gate
  refuses it before decoding; measured on both platforms CI runs.
- A single-frame run's peak and output are unchanged.

## Dependencies

- [Memory preflight & in-place transform](memory-preflight.md) — **done**; the
  per-frame model this extends.
