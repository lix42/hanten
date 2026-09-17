# Sequential encode slows after a wide rayon fan-out

## Goal

Explain, and if warranted remove, the slowdown of the single-threaded stage that
runs right after a wide rayon section. Measured on the 14-core M4 Pro while
landing `output/parallel-display-stages` (2026-09-16, 16.4 MP frame): the
`film-master` f32 TIFF write went from ~96 ms to 150–192 ms, and the `legacy`
u16 encode from 106–115 to 134–144 ms, with the default pool; both returned to
~117–120 ms with `RAYON_NUM_THREADS=4`. Consistent run to run. The u16 presets
recovered when their quantize went parallel in `output/parallel-hdr-stages`;
`film-master`, whose write is inherently sequential, still carries it, so the
interchange master may run slower end to end than before the multithreading work
on some machines.

## What is known and unknown

- Known: the effect follows the rayon thread count, not the amount of parallel
  work — `film-master`'s only parallel stage before the write is the ~5 ms ACEScg
  map. rayon workers sleep on a condvar once a section ends, so it is not
  oversubscription in the ordinary sense.
- Unknown: the cause. Candidates worth ruling out one at a time: the main thread
  being rescheduled onto an efficiency core after the fan-out (macOS QoS; M4 Pro
  has 10 P + 4 E cores), rayon's brief post-section spin, first-touch page
  placement of a buffer written by many threads and then read by one, and the
  TIFF writer's own strip/predictor path. `RAYON_NUM_THREADS=10` (P-cores only)
  did **not** restore the baseline, which argues against the simplest E-core
  story.
- Unknown: whether it reproduces on Linux/x86_64 at all. Measure there before
  designing anything for it.

## A known remainder, in scope if it turns out to be cheap

`quantize_coded_u16` (`io/encode.rs`) is the last sequential per-sample loop on the
coded-HDR TIFF path — roughly 49 M samples on a 16.4 MP frame for `hdr-pq-tiff` and
`hdr-hlg-tiff`. It was deliberately left alone when the other stages were
parallelized: it fuses the quantization map with an `f64` RMS accumulation, and
preserving that sum's exact order would need a full-frame error buffer at 24 B/px,
while banding it would move the reported RMS. Only the `worst` max is order-free. If
this task ends up bounding a rayon pool anyway, re-check whether the parallel-map plus
sequential-reduce split is worth its buffer here.

## How to Verify

- A re-measurement protocol that survives a loaded machine: alternate default and
  `RAYON_NUM_THREADS=1`/`4` runs of `film-master` and `legacy` on the spike frame,
  three each, reading the `--telemetry` `encode` figure; the first attempt at this
  was discarded because the load average was ~40.
- The outcome is either a documented cause with a fix that leaves every preset
  byte-identical, or a documented non-issue. If a knob is the answer, it is an
  operational one (`RAYON_NUM_THREADS` as-is, or a `--threads` flag on the arg struct
  like `--max-memory`), never a recipe key, and the doc must say it cannot change
  bytes — libaom's `AV1_THREADS` is separate and stays pinned.

## Dependencies

- [Parallel HDR stages](parallel-hdr-stages.md)
