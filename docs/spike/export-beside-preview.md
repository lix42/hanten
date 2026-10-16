# Export Beside Preview Spike

**Status:** closed. **Verdict: export can run in the core process beside the preview,
but only with two thread pools and a cooperative pause.**

- **Two pools.** On rayon's shared global pool a preview waits behind export work: p90
  161 ms and p99 275 ms, against 24 and 40 alone.
- **The pause.** With a preview pool, an export pool, and export threads that pause
  between pixel bands while the user drags, the preview stays near its idle latency
  (p99 42 ms against 40). Its tail grew under heavier machine load. The export runs at
  full speed in the rests.
- **QoS.** Lowering the export's macOS QoS is not a priority scheme on Apple silicon: it
  slowed the export 7× or more with no preview running.

**Investigated:** 2026-10-16 · task `app/export-beside-preview-spike` · base `248190f` ·
release build, rustc 1.99.1 · Apple M4 Pro (10 performance + 4 efficiency cores, 48 GB) ·
macOS 26.7.1 · Tauri 2.12.4 (WKWebView), `lcms2` 6.1.1

## Question

The desktop app runs export in the core process as a queue, with the preview taking
priority ([desktop app design](../design/desktop-app.md) §3.6). Does that hold?

1. Preview latency while a roll exports, with and without a priority scheme.
2. How many exports run at once, given memory and cores.
3. Whether export reuses the preview's decode or decodes again.
4. How an lcms2 fault stays with the render that raised it (design §2.7).

## How it was tested

- **Synthetic roll.** Real scans were not reachable (the external drive was not mounted).
  36 frames at 4927×3335, the size of the earlier spikes' frame, were made by
  mirror-tiling the SilverFast crop in `tests/fixtures/hdri-64bit.tif`, RGB and IR plane.
  - Each frame is scaled by its own transmission factor (0.6–1.4×).
  - Each keeps the fixture's XMP, so the decoder reads it as a SilverFast raw scan.
  - Each file is 132 MB, about a real HDRi frame's size.
  - A flat base frame and a flat leader were added, and the roll was measured with
    `measure-roll --unexposed --leader`.
- **The core.** A throwaway branch, `spike/export-beside-preview-core`, never merged: a
  library target with:
  - `preview` (prepare: decode, film base, box downsample to a 2048 edge; render: the
    chain at SDR in Display P3, the sRGB curve applied directly);
  - an export entry point that is `convert`'s own per-frame path (`cli::convert_frame`).
- **Checks.** The in-process export equals `hanten convert --params` in all 49,294,635
  u16 samples. A factor-1 preview equals the export at 8 bits, with lcms2 and with the
  direct curve.
- **The harness.** One process per run, holding the preview and the export queue:
  - **Sweep:** a scripted slider sweep on exposure, one render in flight and the next as
    soon as the previous is done, plus 4 ms for IPC and draw.
  - **Drags:** 1.5 s drags with 1.5 s rests unless stated, so a paused export has time to
    run.
  - **Export queue:** cycles through the roll.
  - **Memory:** peak is the process's lifetime peak physical footprint.
- **The shell.** A minimal Tauri app runs the same sweep in WKWebView against a running
  export. It times input to the next animation frame, as `webkit-display-spike` did.
- **Noise.** Other work was running on the machine (load average 10–58 during the
  runs). The scheduling candidates were interleaved over 3–4 rounds, and the tables
  give medians over rounds. The 1.5× run-to-run spread the webkit spike saw shows here
  too, so read ranges, not digits.

Reference: one `hanten convert` of a synthetic frame takes 0.40–0.43 s wall and 3.3 s
CPU (about 8 cores busy), with a 307 MB peak footprint. The memory model estimates
474 MB.

## Findings

### Q1 — Preview latency during export: needs separate pools and a pause

Preview render times in ms (the render alone: chain, curve and RGBA8 quantize), at a
2048 edge (1642×1111). Each row is the median over interleaved rounds:

| Scheduling | p50 | p90 | p99 | First render of a drag (max) | Exports/s |
|---|---|---|---|---|---|
| No export | 22 | 24 | 40 | 49 | — |
| One shared pool (rayon's global) | 26 | 161 | 275 | 225 | 2.4 |
| Separate pools, export limited to 4 threads | 31 | 35 | 43 | 36 | 1.3 |
| Separate pools, export pauses between stages | 23 | 26 | 54 | 98 | 1.3 |
| Separate pools, export pauses between bands | 23 | 27 | 42 | 29 | 1.2 |

The last two rows come from a 4-round head-to-head at load 10–21. In the pass before it,
at load 23–31, the band gate's round medians were p99 117 ms and first render 117 ms
(rounds of 376, 117 and 26 ms), no better than the stage gate. The export alone runs 2.2–2.5
frames/s, so with half the time spent dragging, about 1.2 frames/s is the whole of the
rest time used.

- **One shared pool fails.** Every `par_*` loop in the pipeline uses rayon's global pool
  (`pipeline::pixels`, the lcms2 transform bands, the quantize), so a preview's chunks
  queue behind an export's.
  - The median holds but the tail does not: p90 rises 7×.
  - With shorter drags (0.5 s, 1 s rest) the drag's first render waits 124 ms at
    median.
- **Two full-width pools without a pause are worse at the median.** In pass 1, 28
  threads on 14 cores gave p50 48 ms and p90 137 ms.
  - Limiting the export to 4 threads keeps the preview steady (p90 35).
  - But it caps the export at 4 cores in the rests too: 1.3 frames/s at 770 ms each,
    where the paused full-width export finishes each in ~350 ms.
- **The stage gate** (an export thread waits at each stage boundary while a preview
  rendered within the last 150 ms):
  - It keeps p50 and p90 at the idle level.
  - A stage already running finishes first. Destination and encode take ~180 ms each
    under contention, so a drag's first render can take ~100 ms.
- **The band gate** adds the same wait between the bands of every parallel loop the
  export runs (16k pixels, or 8 rows for the lcms2 transform):
  - In the head-to-head it held the first render of a drag to 25–32 ms.
  - With 0.5 s drags, p99 32 ms and the first render at most 32 ms, against shared
    p99 298.
  - With nothing to preview it costs nothing measurable: 2.1 against 2.2 frames/s
    (round medians), within noise.
- **Lower QoS for export does not work on Apple silicon.** With no preview running
  (the machine still carried other work):
  - `utility`: 0.33 frames/s, against 2.4 at the default.
  - `background`: one frame in 62 s.
  - The likely cause is the scheduler keeping low-QoS threads on the efficiency cores;
    the harness did not record which cores ran.
  - Under `background` the preview's p90 was 35–54 ms, only because the export barely
    ran.
  - Raising the preview's threads to `user-interactive` beside a `utility` export left
    p99 at 110 ms.
- **A gated export stops while the user drags.** Under a continuous 15 s drag it
  finished 0.07 frames/s. Real drags are short, but a user who never lets go starves
  the queue.

**In WKWebView** (the shell, 60 Hz display, two rounds), input to the next frame in ms:

| Phase | p50 | p90 | p99 | Renders in 6 s of dragging | Exports/s |
|---|---|---|---|---|---|
| No export | 48–49 | 51 | 54–67 | 227–233 | — |
| Export, separate pools and band gate | 49 | 51 | 54–66 | 219–223 | 1.2–1.3 |
| Export, one shared pool | 51 | 184–188 | 316–333 | 76–79 | 2.5–2.6 |

- WebKit's own processes do not stall when the export fills the cores. In the export
  phases, the shared pool included, frame intervals stayed at p99 20–24 ms and at most
  36 ms, against p99 18–20 ms idle. IPC stayed at 2.1–2.8 ms.
- On the shared pool the canvas updated a third as often, which is what the user would
  see.

### Q2 — How many at once: one

The queue alone, no preview:

| Exports at once | Frames/s | Wall per frame (p50) | Peak footprint |
|---|---|---|---|
| 1 | 2.5 | 383 ms | 309 MB |
| 2 | 2.9 | 666 ms | 611 MB |
| 3 | 3.3 | 863 ms | 908 MB |

- **One export already fills most of the machine** (3.3 s of CPU per 0.4 s frame). A
  second adds 16% throughput for twice the memory.
- **Beside a gated preview, a second export gains nothing.** Two gave 1.1 frames/s
  against 1.2–1.3 for one, and 636 MB against 361 MB.
- **Each export adds ~300 MB.** `pipeline/memory.rs` predicts 474 MB for one frame of
  this size, an over-estimate on the safe side.
- **The preview adds little at rest.** A prepared frame adds ~36 MB. Prepare briefly
  holds a full decode, so a preview's peak is ~304 MB. With one export running, the
  process peaked at 340–361 MB in the harness and 397 MB in the shell.
- **Not measured:** preparing a frame while an export runs. It would stack the two
  full-frame peaks (~600 MB).

### Q3 — Reuse the preview's decode: no, decode again

- **Decode is a small share of an export.** It took 44 ms of a 383 ms export (11%),
  warm. In a contended run (670 ms wall) the medians were:

  | decode | film base | reconstruction | scene correction | look | fit range | fit gamut | destination | encode |
  |---|---|---|---|---|---|---|---|---|
  | 54 | 14 | 34 | 46 | 69 | 19 | 21 | 181 | 176 |

- **Reuse would mean keeping the full decode.** The preview's cache point is the
  downsampled scan (design §3.5). Keeping the full decode costs 16 B/px, 263 MB per
  16 MP frame. The decode is also the buffer the chain consumes in place, so sharing it
  means a copy or holding it until export.
- **The file read is warm either way.** The preview's read brings the file into the
  page cache, and the export re-reads it from there. Whether that holds for a cloud
  folder or a roll larger than the cache is `app/preview-cache-spike`'s question.

### Q4 — lcms2 faults: one context per render, its own handler

A two-thread race of "clear the flag, transform 64k pixels, check the flag", as
`cli::render_frame` does, 4,000 renders per thread. One thread never faults; the other
raises an lcms2 error from a rayon worker in every other render (2,000 faults):

| Scheme | Clean thread's renders that saw a fault | Faulty renders that saw none |
|---|---|---|
| Today's process-global handler and flag | 935 of 4,000 | 937 of 2,000 |
| One `ThreadContext` per render, handler keyed by context handle | 0 | 0 |

- Faults on a context with its own handler never reached the global handler.
- **A context without a handler of its own drops its faults silently.** This covered a
  real ICC parse fault too. It does not fall back to the global handler, so every
  context must set one.
- A fault raised on a rayon worker reached the right render, because the handler is
  found by the context, not the thread. A thread-local flag would not work: the fault
  fires on a pool thread.
- The safe `lcms2` crate does not export the context's raw handle (its `Context` trait
  is hidden). The spike read it through `ThreadContext`'s `#[repr(transparent)]`
  layout. A real version would create the context through `lcms2-sys` with user data,
  or keep a registry.

## Recommended scheduling

1. **Two rayon pools, both full width:** a preview pool and an export pool, each
   entered with `ThreadPool::install` so every nested `par_*` loop of a stage runs in
   it. Neither uses the global pool.
2. **Default QoS for both.** Never lower the export's.
3. **A cooperative pause:**
   - Export threads wait at each stage boundary and between bands of every parallel
     loop while the preview has rendered in the last 150 ms.
   - The preview marks itself active at the start and end of each render.
4. **One export at a time.**
5. **Export decodes its frame again;** the preview cache keeps only downsampled scans.
6. **lcms2:**
   - Each render job (a preview, an export) has its own `ThreadContext`, with a handler
     that records into that job's report channel.
   - The process-global handler goes, and no context is left without a handler.

## Unknowns this spike turned up

1. **The pause has to reach every parallel loop an export runs.** In the spike these are
   `pipeline::pixels`'s drivers, `color::transform_in_place`, `near_black::measure` and
   the u16 quantize. A loop added without the hook brings the tail back. How a pure
   stage receives an operational hook (like `StageClock`) is design work.
2. **QoS is not priority on Apple silicon.** A lower class slowed the export 7× or more,
   likely because it keeps work on the efficiency cores. Any later "background" work in
   the app (measuring, a disk cache) would pay the same. Re-time it on a quiet machine
   before relying on the exact factor.
3. **A user who drags non-stop starves the queue.** Accept it, or give the export a
   floor (a band every so often).
4. **Moving `pipeline::color` to per-job contexts touches profile construction too.**
   Profiles and transforms must share a context. `synth_coded_hdr`'s raw `lcms2-sys`
   calls use the null global context today. The cost of creating a context per render
   was not timed.
5. **The first render after a rest is slower even with nothing exporting** (40–50 ms
   against 22). That is part of the preview's budget, not the export's.

## Not settled

- Real scans: the frames are synthetic, decode is warm from the page cache, and no file
  was read cold from a cloud folder.
- Smaller Macs (fewer cores, 8–16 GB), a 3000 edge, and thermal behaviour over a whole
  roll (each run lasted at most 21 s).
- Only the SDR TIFF export ran beside the preview, and only the SDR preview. The HDR
  preview, a gain-map export (two renditions, more memory) and measuring a roll in the
  core beside the preview were not run.
- The hold window: 150 ms throughout; one run at 50 ms looked the same.

## How to rebuild

Everything is in `../temp/export-beside-preview-spike/`. The roll and outputs are in
`.noindex` folders, so Spotlight skips them.

- **Roll.**
  - `core/examples/make_roll.rs` writes `roll.noindex/` from the fixture: `make_roll
    <fixture> <dir> 36 4927 3335`.
  - `measure.sh` measures it into `roll.json` and times one CLI convert.
- **Core.** Branch `spike/export-beside-preview-core` from `248190f`, a worktree at
  `core/`, uncommitted and never merged:
  - `src/lib.rs` takes `main.rs`'s module list and adds `preview` and `spike`.
  - `src/cli/spike_api.rs` wraps `convert_frame`, `read_layer` and the lcms2 handler.
  - `src/preview.rs` has resolve, prepare and render.
  - `src/spike.rs` holds the gate, QoS and footprint helpers.
  - Hooks: `spike::before_stage()` in `StageTimer::time`, and `spike::before_band()` in
    `pipeline::pixels`, `color::transform_in_place`, `near_black::measure` and
    `encode::quantize_u16`.
- **Harness and checks.**
  - `examples/export_beside_preview.rs` takes `key=value` arguments (variant, exports,
    threads, QoS, drag pattern) and prints one JSON line.
  - `run.sh` … `run5.sh` are the passes, `aggregate.py` the round medians, and
    `results-*.jsonl` the raw results.
  - `examples/preview_check.rs` (`check.sh`) holds the equality checks.
  - `examples/lcms_fault.rs` is the fault race.
- **Shell.** `shell/` is a plain Cargo crate with its own `[workspace]`, depending on
  `nc` by path:
  - Tauri's `custom-protocol` feature bundles `www/`, so no dev server is needed.
  - Commands are `#[tauri::command(async)]`, off the main thread.
  - `shell.sh` runs every phase unattended (`SPIKE_AUTO=1`) into `shell-results.json`.
