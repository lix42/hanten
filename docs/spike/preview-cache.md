# Preview Cache Spike

**Status:** done. **Verdict: cache the downsampled scan as f16, decode each frame once
for both measuring and the preview, and keep a disk cache.**

- **One decode.** A roll that is not yet measured needs every frame decoded for
  measuring anyway. Downsampling that same decode into the cache costs a few ms a frame,
  and `roll.json` comes out identical.
- **Measuring per frame, in parallel.** Measuring takes 8.8 s alone. On 2–3 threads,
  each frame measured on its own and the results merged in input order, it takes
  5.1–6.6 s, with a `roll.json` equal to the CLI's.
- **f16 at a 2048 edge.** That is 11 MB a frame, 0.4 GB for 36 frames, and 1.7 ms to
  widen before a render.
- **A disk cache.** It reopens the whole roll in 0.1 s, against 2.3 s to decode it again
  from a fast SSD, and far longer from a cloud folder.

**Investigated:** 2026-10-16 · task `app/preview-cache-spike` · base `370ac04` · release
build, rustc 1.99.1 · Apple M3 Pro (5 performance + 6 efficiency cores, 18 GB) ·
macOS 26.6.2 · roll `rolls/2026-09-20-Portra400` (34 pictures plus base and leader,
4946×3346, 142 MB each) on an external PCIe SSD

## Question

The app keeps each frame's downsampled scan in memory, so a knob change only renders
([desktop app design](../design/desktop-app.md) §3.5). Earlier spikes timed one frame.
Over a whole roll:

1. Time to the first preview and to a fully measured roll, warm and cold.
2. What one decode shared by measuring and the preview saves.
3. A memory budget, and what is evicted first.
4. Whether a disk cache of downsampled scans makes reopening a roll quick enough to keep.

## How it was tested

- **The core.** A throwaway branch, `spike/preview-cache-core`, never merged: a library
  target with
  - `preview`: prepare (decode, box downsample to the max edge, f32 or f16) and render
    (the chain at SDR in Display P3, the sRGB curve applied directly);
  - a hook in `decode_for_roll_white` that hands each picture frame's decode to the
    cache before the fixed decode consumes it;
  - `measure-roll`'s per-frame loop as a function of one frame, run on 1–3 plain
    threads and merged in input order (`NC_SPIKE_MEASURE_THREADS`);
  - `run_args`, the CLI's `convert`, `measure-roll` and `measure-base` in-process.
- **Checks.**
  - In-process `measure-roll`, the shared decode and the per-frame threads all write a
    `roll.json` equal to the CLI's (the shared run takes frames in the app's order, from
    the current frame outward).
  - f32 against f16 scans, every frame rendered to RGBA8: see Q3.
- **The harness.** One process per run, printing one JSON line:
  - **Opening a measured roll:** a `roll.json` exists; workers prepare every frame, the
    current one (frame 18 of 34) first and then outward, and the current one is
    rendered as soon as it is ready.
  - **Opening an unmeasured roll:** measuring and the preview, in the arms of Q2.
  - **Memory:** a 5 ms footprint sampler gives the peak while opening, and the
    footprint once every frame is cached.
- **Warm and cold.** Warm runs read every file first. Cold runs followed `sudo purge`.
  This SSD reads a frame in 44 ms cold (3.2 GB/s) against 10 ms warm, so a cold read
  adds ~35 ms a frame here. That is inside the run-to-run noise, and the cold runs
  happened to meet a quieter machine. A cloud folder is the cold case that matters, and
  it was not run here: ~0.75 s a frame from Google Drive (`native-app-prototype`) and
  1.3 s from iCloud Drive (`file-access`).
- **Noise.** Other work kept the load average at 4–50. Variants were interleaved over
  2–3 rounds, and tables give the median with the range.

## Findings

### Q1 — Opening a roll: first preview in 0.2–0.4 s, all 34 frames in 2.3–3.2 s

A measured roll (`roll.json` exists), 2048 edge, f32. "First preview" is the current
frame decoded, downsampled and rendered:

| Workers | First preview, warm (ms) | All frames, warm (s) | First preview, cold (ms) | All frames, cold (s) | Peak while opening |
|---|---|---|---|---|---|
| 1 | 158 (141–345) | 4.3 (4.2–5.1) | 213 (119–307) | 3.6 (2.8–4.4) | 1.03 GB |
| 2 | 391 (183–798) | 3.2 (2.9–4.4) | 175 (150–200) | 2.3 (2.1–2.5) | 1.24–1.28 GB |
| 3 | 316 (209–430) | 2.9 (2.4–3.0) | — | — | 1.51 GB |
| 4 | 485 (301–530) | 2.5 (2.4–3.4) | 317 (246–388) | 2.3 (1.9–2.7) | 1.66–1.69 GB |

- **Decode dominates the prepare.** At 1 worker the decode takes 94 ms, the downsample
  16 ms and the render 45–75 ms. Each decode runs slower as workers are added (150 ms at
  2, 226 ms at 4), so a worker past 2 buys little time and costs up to ~265 MB, one
  full decode in flight.
- **The first preview on its own is one decode and a render**, 0.15–0.4 s from this
  SSD. From a cloud folder add 0.75–1.3 s of read before the decode.

### Q2 — One decode shared by measuring and the preview

An unmeasured roll, warm unless stated:

| Arm | First preview | All previews | Roll measured | Peak |
|---|---|---|---|---|
| Measure, then prepare (the prototype) | 8.9 s (8.2–15.5) | 11.8 s | 8.7 s | 1.42 GB |
| Base first, then measuring beside 2 prepare workers (decoded twice) | 0.47 s (0.38–3.5) | 4.8 s (3.9–9.3) | 13.4 s (9.8–27.1) | 1.52 GB |
| … cold | 0.34 s (0.32–0.35) | 2.9 s (2.6–3.1) | 8.3 s (7.7–8.9) | 1.54 GB |
| Shared decode, frames measured one at a time | 0.4–0.6 s | with measuring | 8.3 s (6.8–8.5) | 1.16 GB |
| Shared decode, 2 measuring threads | 0.4–0.6 s | with measuring | 6.4 s (6.1–6.9) | 1.41 GB |
| Shared decode, 3 measuring threads | 0.45–0.7 s | with measuring | 5.2 s (5.0–6.7) | 1.60 GB |
| Measuring alone, one at a time | — | — | 8.8 s (6.3–10.9) | 0.42 GB |
| Measuring alone, 2 / 3 threads | — | — | 6.6 s / 5.1 s | 0.69 / 0.96 GB |

- **Sharing the decode costs measuring nothing.** The downsample adds 4–9 ms a frame
  beside measuring's ~190 ms. Holding the 750 MB cache did not slow it either (a run
  that dropped each scan timed the same).
- **What it saves.**
  - A second read of every frame: 4.8 GB for this roll. From iCloud Drive the first
    read downloads the file whole, so the second is local unless the file was evicted
    in between.
  - A second decode of every frame: ~3–5 s of CPU.
  - Peak memory against decoding twice in parallel (1.52 GB): 0.36 GB with one
    measuring thread, ~0.1 GB with two.
  - Not measuring time on its own. One measuring thread took 8.3 s, as long as
    decoding twice did cold. The faster measuring (6.4 s) comes from the second
    measuring thread, which helps measuring alone just as much (6.6 s).
- **What it costs.** Previews arrive at measuring's pace, about 5–6 s for the roll,
  rather than the 2.3–3 s a prepare-only pass takes. A frame's measuring does more than
  its prepare: the fixed decode at full size, the working-space map, and sampling.
- **The first preview waits for the base**, because the chain needs it to render. In the
  spike, measuring decoded the base, then the leader, then the current frame (~0.4 s).
  The leader is only needed when the pool is guarded, so the current frame could come
  before it. Not tested.
- **Measuring on one thread leaves cores idle.** One frame at a time keeps ~1.2 cores
  busy (the CLI used 20 s CPU in 16.6 s wall), because the decode is mostly serial.
  - Plain threads, each taking whole frames, measured the roll in 6.6 s (2 threads) and
    5.1 s (3), against 8.8 s.
  - Running the frames on a confined 2-thread rayon pool made it slower, not faster
    (11.2 s): it confines every nested `par_*` loop of the frame to those 2 threads.
    This is the trap the export spike's pools hit too.

### Q3 — Memory: a whole roll fits at f16; each decode in flight costs ~265 MB

Cache size per frame, and render cost, on this roll:

| Edge | Preview size | f32 per frame | f16 per frame | 34 frames at f16 | Render (ms) | f16 widen (ms) |
|---|---|---|---|---|---|---|
| 2048 | 1646×1111 | 22 MB | 11 MB | 376 MB | 44–74 | 1.4–2.0 |
| 3000 | 2469×1666 | 49 MB | 25 MB | 846 MB | 108–135 | 4.0–4.4 |

- **f16 is enough for a preview.** Over all 34 frames, 1.8% of the 8-bit samples
  differed from the f32 render, by at most 2 levels. A preview is never bit-equal to the
  export anyway, since every stage sees box-averaged pixels.
- **Transient memory stacks on top of the cache:**
  - each frame decoding: up to ~265 MB, one full decode at 16 B/px with the IR plane
    (each added worker raised the peak 150–270 MB);
  - each measuring thread: ~270 MB;
  - an export: ~300 MB (export spike), seen here as +0.24–0.3 GB while opening;
  - the process itself: ~20 MB.
- **The footprint returns to the cache size at rest.** With every frame cached, it
  settles at cache bytes + ~15 MB (768 MB for 753 MB of f32 scans). Freed decodes go
  back to the OS (`allocator.rs` unmaps big blocks).
- **A 3000 edge is too slow to drag on this Mac.** Renders take 108–135 ms, against
  44–74 ms at 2048.

### Q4 — A disk cache: keep it

The f16 and f32 scans of the whole roll, written as one file per frame (a JSON header,
then raw samples), read back with 2 workers:

| | Write the roll | Size | First preview, cold | All 34 frames, cold |
|---|---|---|---|---|
| f16 | 0.45–0.89 s | 376 MB | 64–73 ms | 112–125 ms |
| f32 | 0.74–1.27 s | 753 MB | 92–98 ms | 177–209 ms |

Cold here means read from the internal drive after `purge`.

- **Against decoding again:**
  - 2.1–2.5 s from this external SSD;
  - 25–44 s of reads when a cloud folder's files are not local, at 0.75–1.3 s a frame
    (an estimate, not run);
  - each read takes 4–8 ms against a 94–230 ms decode.
- **An evicted frame comes back almost free.** With a disk cache, evicting from memory
  costs a few ms on return, so the memory budget can be small.
- **The cached scan depends only on the file:**
  - It is the decoded transmission before the film base. A new base, a re-measured roll
    or any knob does not make it stale.
  - A crop could apply at render to the downsampled scan, so a crop change would not
    make it stale either. Not tested.
  - The spike keyed each entry by path, size, mtime (ns) and inode. Writes went through
    a temporary file and a rename, so a half-written entry is never read.

### Opening a roll while an export runs

Warm, 2 workers, `convert` of other frames looping in-process with no pause (not the
export spike's gated scheme):

| | First preview | All frames | Peak | Exports finished |
|---|---|---|---|---|
| No export | 0.39 s (0.18–0.80) | 3.2 s (2.9–4.4) | 1.24–1.31 GB | — |
| Export running | 0.46 s (0.23–2.35) | 5.0 s (4.1–7.1) | 1.48–1.55 GB | 6 per run |

- **The peaks stack as expected** (+0.24–0.3 GB). The export spike's ~600 MB estimate
  was for one prepare beside one export; two prepare workers and an export reach
  ~1.5 GB with the cache.
- **The first preview can wait behind the export.** One run took 2.35 s.

## Recommended cache design

1. **The cache point is the downsampled scan**, before the film base: decoded
   transmission, box-averaged to a 2048 edge, stored as f16. Key it by the file's
   identity (path, size, mtime, inode) and a version of the decode and downsample, never
   by the recipe.
2. **Open a measured roll with 2 prepare workers**, the current frame first, then
   outward. More workers add up to ~265 MB each and little time on this Mac.
3. **Open an unmeasured roll with one decode per frame.** It feeds both the downsample
   and that frame's measurement:
   - The frames' measurements run on the same 2 workers, plain threads with nested loops
     on the global pool, and are pooled in input order at the end. That ordering keeps
     `roll.json` identical to `measure-roll`'s.
   - The base is decoded first, so the current frame's preview can render (try the
     current frame before the leader).
4. **Memory:**
   - The cache holds the whole roll in memory when it fits a budget: 0.4 GB for 36
     frames at f16.
   - Over budget (long rolls, small Macs), evict the frame furthest from the current
     one, never the current frame or its neighbours.
   - Transient memory is ~265 MB per frame decoding plus ~300 MB for an export.
     `pipeline/memory.rs` needs a session model of that sum (cache bytes + workers ×
     one frame's decode + one export) to size the workers and refuse early.
5. **A disk cache** in `~/Library/Caches`: f16 entries written after each prepare,
   atomically. It turns a reopen into ~0.1 s and makes eviction cheap. It needs a size
   cap (this roll is 376 MB) and an eviction order of its own.

## Unknowns this spike turned up

1. **Prepares beside an export need a priority too.** Only renders pause the export in
   the export spike's scheme. The current frame's prepare is user-facing (one waited
   2.35 s here); the neighbours' are background. Which pool each runs in, and whether
   the pause covers them, is design work.
2. **The base comes before any preview** of an unmeasured roll. Whether the current
   frame can come before the leader (whose guard applies only at pooling) was not
   tested.
3. **A session memory model.** `pipeline/memory.rs` estimates one run; the app holds a
   cache, prepare workers, measuring threads and an export at once.
4. **Disk cache housekeeping:** its size cap, when a roll's entries go, and whether an
   entry keyed by path survives a drive mounted under another name (the inode does).
5. **Measuring's pooling order is load-bearing.** Parallel frames must merge in input
   order, or the pool's percentiles move and `roll.json` changes.

## Not settled

- Opening a roll from a cloud folder: the per-frame reads are other spikes' (Google
  Drive in `native-app-prototype`, iCloud in `file-access`). A file Google Drive has
  only partly streamed, or an iCloud file read offline, is `io/truncated-file-fault`'s
  case.
- Smaller Macs (8 GB, fewer cores) and a larger roll; the HDR preview and a 3000 edge.
- Crop applied at render to the cached scan.

## How to rebuild

Everything is in `../temp/preview-cache-spike/` (on the SSD,
`/Volumes/square/hanten/temp/preview-cache-spike/`):

- **Core.** Branch `spike/preview-cache-core` from `370ac04`, a worktree at `core/`,
  uncommitted and never merged:
  - `src/lib.rs` takes `main.rs`'s module list and adds `preview` and `spike`.
  - `src/spike.rs`: the scan hook and the footprint probe (`proc_pid_rusage`).
  - `src/preview.rs`: prepare, downsample, f16 conversion and render.
  - `src/cli.rs`:
    - `crate::spike::on_scan` in `decode_for_roll_white`;
    - `measure_roll`'s picture loop as `measure_one`, threaded by
      `NC_SPIKE_MEASURE_THREADS`;
    - `run_args` and `spike_frame_recipe`.
- **Harness.** `core/examples/roll_open.rs`, `key=value` arguments (`mode`, `workers`,
  `edge`, `store`, `current`, `cache`, `keep`, `ordered`), one JSON line per run.
- **Scripts.**
  - `measure-cli.sh`: the CLI reference `roll.json`.
  - `check.sh`: the in-process measurement against it.
  - `warm.sh`, `cold.sh` (needs `sudo -v` in a terminal), `shared.sh`, `parmeasure.sh`:
    the passes.
  - `readspeed.sh`: raw cold and warm reads.
  - `aggregate.py`: medians over rounds.
- **Results:** `results-*.jsonl`, and the scratch outputs in `out.noindex/`. The disk
  caches for the cold runs are in `~/Library/Caches/hanten-preview-cache-spike/`.
