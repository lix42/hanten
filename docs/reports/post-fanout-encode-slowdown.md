# Sequential encode after a rayon fan-out (2026-10-13)

Measured for `output/post-fanout-encode-slowdown`: does the single-threaded encode
that follows a wide rayon section run slower with more rayon threads? **On an M3 Pro,
no.** The M4 Pro observation that filed the task (`film-master` encode 96 → 150–192 ms
with the default pool, ~117 ms at `RAYON_NUM_THREADS=4`) was not re-measured on that
machine, and Linux was not measured.

## Setup

- Apple M3 Pro, 5 performance + 6 efficiency cores, 18 GiB, macOS 26.6; release build
  of `bd3f8af`.
- Frame `rolls/2026-09-13-Portra400/1675.tif` (4927x3335, 16.4 MP),
  `--film-base 0.3199054,0.14962997,0.081757836`, as in
  [gpu-rendering-spike](../spike/gpu-rendering-spike.md).
- Probes: `--film-master` (f32 TIFF, 197 MB); `--range hdr --transfer pq --container
  tiff` (sequential `quantize_coded_u16` after parallel stages); `--range sdr
  --container tiff` as a control (its quantize is parallel).
- The telemetry `encode` stage, plus a throwaway build timing its parts.

## Protocol

146 runs in three matrices: a fixed-order one (5 rounds × 6 arms × the 3 probes = 90),
a shuffled film-master one (8 rounds × the 4 arms below plus a 100 ms sleep = 40), and
the quantize comparison (8 × 2 = 16).

- **Gate every run on the 1-minute load average** (< 4 here, on 11 cores), checked
  immediately before the run; every run started at 3.2–4.0.
- **Shuffle the arm order every round.** In a fixed order the arms that ran last in a
  round looked uniformly fast, because slow writes cluster at round starts.
- **Write the output to the internal disk.** `encode` includes the staged write's
  `fsync` (since 2026-08-04), so an output on an external volume measures that
  volume.

## Results

Film master, 8 shuffled rounds, median [min–max] in ms:

| arm | encode | non-finite scan | channel means (f64) | TIFF write | fsync |
|---|---|---|---|---|---|
| default (11 threads) | 108 [94–186] | 7 | 36 | 32 [30–92] | 20 |
| `RAYON_NUM_THREADS=1` | 112 [104–180] | 7 | 37 | 36 [31–54] | 29 |
| `RAYON_NUM_THREADS=4` | 117 [101–204] | 7 | 37 | 36 [30–78] | 27 |
| `RAYON_NUM_THREADS=5` | 121 [101–150] | 7 | 35 | 34 [31–72] | 29 |

- **No step depends on the thread count**, including the one-thread arm, where nothing
  fanned out wide before the encode.
- **The TIFF write has slow episodes that are system state.** In the first,
  fixed-order matrix the write took 130–325 ms in three of five rounds, in every arm
  of the round including `RAYON_NUM_THREADS=1`, fading within the round; at ~30 ms it
  is a 197 MB copy into the page cache, and the machine had ~75 MB free with ~10 M
  page-outs since boot. That is the likeliest reading of the original M4 Pro figures
  too, but it is an inference: those runs were not interleaved.
- **The main thread already runs at `QOS_CLASS_USER_INTERACTIVE`**, so it is not
  demoted after the fan-out.
- **A 100 ms sleep before the encode slows the sequential passes** (scan 7 → 10,
  means 36 → 40 ms): the core clocks down while idle. A post-burst frequency penalty
  would show the opposite.

## `quantize_coded_u16` in parallel

The coded-HDR quantize can go parallel without the full-frame error buffer the task
file assumed: map the codes in parallel (lowest-index domain error wins), then
recompute each error from the sample and its stored code in a sequential pass, in the
original order, so the reported RMS is unchanged. Measured on `hdr-pq-tiff`, 8
shuffled rounds: **68 → 58 ms** (map 8 ms, sequential reduce 42 ms, channel means the rest). The sequential
`f64` accumulation is the floor, so the saving is ~10 ms of a ~480 ms run. Not taken.

## Verdict

A non-issue on this machine; no knob or fix. Reopen only with an interleaved,
load-gated measurement on the affected machine that shows the effect by arm.
