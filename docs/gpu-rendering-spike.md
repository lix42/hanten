# GPU Rendering Spike

**Status:** closed — decision recorded; revisit when nc gains an interactive
native app or a browser (WASM) build.

**Investigated:** 2026-09-16 · `nc` at `6643a23`, release build, rustc 1.98.1 ·
Apple M4 Pro (14 cores, 48 GiB)

## Decision

1. **Do not port the pipeline to the GPU for the CLI.** On a real 150 MB scan the
   math a shader could take over is under 10% of an HDR run and at most ~40% of a
   TIFF run *after* the CPU fix below, and the transfer of ~200 MB of f32 pixels
   each way eats most of what is left.
2. **Multithread the CPU stages instead.** Everything after reconstruction is
   single-threaded today. A rayon pass over eight per-pixel loops produced
   byte-identical output and cut the TIFF and gain-map presets 3–4x.
3. **Let libaom use row multithreading with a fixed thread count ≥ 2.** Output is
   identical for 2, 4 and 8 threads and 5x faster at 8; only the one-thread case
   differs, so the pinned single thread is a choice, not a format constraint.
4. **GPU becomes the right tool the moment the product is interactive**, and is
   the main path in a browser. The stages to move are the per-pixel ones; the
   reductions stay on the CPU.

## Where the time goes today

Frame: `rolls/2026-09-13-Portra400/1675.tif` (manifest role `real`), 4927x3335 =
16.4 MP, 64-bit HDRi with IR plane, 134 MB — the 150 MB class. Film base measured
from the roll's `base.tif` (`--base-region 2000,1400,800,600`):
`--film-base 0.3199054,0.14962997,0.081757836`. Warm file cache. Stage timings from
`--telemetry`, in ms.

| Preset | decode | algorithm | colour + display | encode | total |
|---|---|---|---|---|---|
| legacy | 34 | 88 | 1141 | 106 | 1377 |
| film-master | 34 | 96 | 0 | 96 | 235 |
| display-p3 | 35 | 107 | 1321 | 100 | 1567 |
| hdr-linear-tiff | 34 | 105 | 196 | 119 | 463 |
| gain-map-hdr | 36 | 105 | 649 | 1489 | 2285 |
| hdr-pq | 33 | 105 | 565 | 2824 | 3532 |

- *algorithm* is the telemetry bucket of that name: reconstruction plus
  `finish_print` on `legacy`, or plus the ACEScg mapper and the shared print
  controls on every other preset. Reconstruction is the only part of it using
  rayon (`algo/density.rs`, `algo/simple.rs`, `algo/film_stock`); it is not timed
  on its own, so the ~90 ms is an upper bound for it.
- Every other per-pixel loop is sequential: the lcms2 transform in
  `pipeline/color.rs`, `sdr::render`, `hdr::render`, `hdr::encode_transfer`,
  `gain_map::build`, `io::encode::quantize_u16`, and inside the *algorithm*
  bucket `render_split::apply_shared_controls` and
  `working_space::map_nc_film_rgb_v1`. The single largest item is the
  lcms2 transform (~1100 ms), which runs on `legacy`, `display-p3` and the
  gain-map preset's SDR base (there it is booked under *encode*).
- The HDR presets are encoder-bound: libaom 2.8 s, the gain-map JPEG path 1.5 s.
- Everything is per-pixel, so ratios are size-invariant. The 18.7 MP
  `Portra4000-2026-08-05-positive/20260807-film-1325.tif` scaled every stage by
  ~1.14x. A cold read of the 134 MB file from Google Drive took ~750 ms, about
  twice a whole `legacy` run after Experiment 1.

## Experiment 1 — CPU multithreading (throwaway patch, reverted)

Rayon over the eight loops above. The lcms2 transform was built with
`Flags::NO_CACHE` (the crate's constructors always return the cache-allowing type,
so a `Sync` wrapper or one transform per thread is needed) and run over row chunks.
The MaxFALL luminance sum in `hdr::render` is a floating-point reduction whose order
matters; it was kept as a sequential pass over the rendered buffer. Min/max and the
integer clip counters are order-free.

| Preset | sequential | parallel | bytes |
|---|---|---|---|
| legacy | 1377 | 360 | identical |
| film-master | 235 | 296 | identical (path untouched; noise) |
| display-p3 | 1567 | 417 | identical |
| hdr-linear-tiff | 463 | 358 | identical |
| gain-map-hdr | 2285 | 882 | identical |
| hdr-pq | 3532 | 3080 | identical |

Gain-map encode after the patch, 471 ms total: legacy gain-map encode 93,
transfer encode 113, quantize 126, base JPEG (`jpeg-encoder`, pure Rust) 121,
gain-map JPEG 7, packaging 4. The first three are nc loops and parallelize the
same way.

Caveat: the patch collected into temporary vectors and flattened, which raised the
gain-map preset's peak RSS from 1.49 to 2.28 GB. A real implementation writes in
place; any added buffer must be reflected in `pipeline/memory.rs`.

## Experiment 2 — libaom row multithreading (`hdr-pq`)

`io/avif.rs` pins `g_threads = 1` and `AV1E_SET_ROW_MT = 0` for determinism.
Measured on the same frame:

| threads | row-mt | encode (ms) | output |
|---|---|---|---|
| 1 | off (shipped) | 2824 | A, 2,403,249 bytes |
| 1 | on | 2842 | A |
| 2 | on | 1481 | B |
| 4 | on | 887 | B |
| 8 | on | 570 | B, 2,405,952 bytes (+0.11%) |

Each configuration was run twice with identical bytes. libaom treats one thread as
row-mt off, so the deterministic rule is *row-mt on, fixed thread count ≥ 2, pinned
rather than derived from the machine*. Shipping it changes today's bytes, so it
needs a baseline update and a CI test hashing a 2-thread and an 8-thread encode on
Linux. Verified on one build and machine only.

## Why a GPU port does not pay off for the CLI

**Amdahl.** After Experiment 1 the GPU-eligible math is ~150–350 ms per 16 MP
frame against a 0.35–0.9 s run (0.8 s for `hdr-pq` after Experiment 2). Uploading
and downloading ~200 MB of f32 RGB costs tens of ms even on unified memory. The
realistic saving is 100–250 ms per frame.

**Determinism.** nc hashes raw f32 bits (`version::PIPELINE_FINGERPRINTS`) and
bounds cross-platform drift with a 1-ULP libm window (`stages::golden`). GPUs give
no such guarantee:

- WGSL specifies `exp2` at `3 + 2|x|` ULP, `log2` at an absolute error of 2^-21 on
  [0.5, 2] and 3 ULP outside, and `pow` only as "inherited from
  `exp2(y * log2(x))`". Implementations may reassociate and fuse. A density curve
  is `log10` then `10^x`, and the golden tests already show 1 ULP of density
  amplified 62x at the pixel.
- Metal compiles shaders with fast-math on by default; `wgpu-hal`'s Metal backend
  creates `MTLCompileOptions` and sets only the language version, invariance and
  logging, so there is no knob to turn it off through wgpu.
- Results vary by GPU vendor and driver, a new axis beyond build/architecture.
- lcms2 cannot run on a GPU; reproducing the legacy transform as shader math
  moves the frozen v0 baseline.

The user's position (2026-09-16): cross-vendor bit-identity is *not* critical for a
GPU path; a tolerance would be defined. Same-machine run-to-run identity remains in
the spec, which per-pixel kernels deliver and naive reductions do not.

**Engineering.** wgpu's default limits are 128 MiB per storage binding and 256 MiB
per buffer, so a 16 MP f32 plane (197 MB) already needs chunking and a 74.6 MP scan
eight or more chunks. CI runners have no GPU (lavapipe only), so a CPU fallback stays
mandatory: two implementations of every stage, plus the wgpu/naga dependency tree.

## When the GPU does pay off

### Interactive native app (preview, knobs, pick-one-of-N)

A preview is 2–4 MP, and most knobs touch only the display stage; decode, film
base and reconstruction are cached per image. Display math measured 4–18 ms/MP on
14 cores after Experiment 1. Per knob change on a 3 MP preview (first row measured
and scaled; others estimated):

| Path | per change |
|---|---|
| CPU, 14 cores | 15–55 ms |
| CPU, 4-core laptop | 50–190 ms |
| GPU, any machine | < 5 ms, rendered to screen, no readback |

The CPU is already interactive on a strong machine; pick-one-of-five costs a few
hundred ms in total. The GPU earns its place for slider dragging, low-core
machines and 100% zoom (frame resident on the GPU, only visible tiles rendered).
Its benefit is latency and machine independence, not throughput.

Two costs remain either way: the final download is a full-resolution render plus
a CPU encode (TIFF ~100 ms, gain-map JPEG ~470 ms, AVIF ~570–800 ms), and a GPU
preview with a CPU final means the user chooses from an image that is not exactly
the one delivered. Either both use the same kernel or a tolerance is stated.

### Browser build (WASM, Chrome assumed)

- **Threads are gated.** rayon via `wasm-bindgen-rayon` needs SharedArrayBuffer,
  which requires a cross-origin-isolated page (COOP/COEP headers) and a nightly
  toolchain for the atomics target.
- **Single-threaded WASM is not interactive here.** Native sequential display math
  was 1.1–1.3 s on 16.4 MP; at a 1.5–2x WASM penalty a 3 MP preview is ~400–500 ms
  per change without threads, ~50–80 ms with 8 workers.
- **WebGPU is mature in Chrome.** wgpu targets it; compute shaders and f32 storage
  buffers work; the preview renders to a canvas with no readback. Buffers above
  the default limits need requested limits or tiling.
- **Memory is a constraint.** wasm32 allows 4 GB. The pixel data alone is small
  (a 150 MB scan is 131 MB decoded + 197 MB f32), but the CLI's buffer lifetimes
  peak far higher (`pipeline/memory.rs`): 0.9–1.7 GB measured on a 16–19 MP frame,
  3.6 GB (SDR) to 5.9 GB (`hdr-pq`) on a 74.65 MP scan. A 150 MB scan fits; a 74 MP
  scan does not without streaming or shorter buffer lifetimes. A preview-resolution
  path sidesteps this until the final export.
- **Encoders get worse and the GPU cannot help.** libaom, libjpeg-turbo, libultrahdr
  and lcms2 are C (emscripten/wasi toolchain). Without threads a 16 MP AVIF encode
  is roughly 5–10 s. TIFF is pure Rust and cheap. lcms2 is replaceable: nc builds
  matrix-and-curve profiles in code, and that math is a small shader.

In the browser the GPU is the realistic path to an interactive preview; CPU
threads are an enhancement where the host allows isolation.

## Architecture for the multithreading work

Decided 2026-09-16, after asking whether a parallel-compute layer (a wrapper over
rayon, or an executor abstraction a GPU backend could later implement) should be
added first. **No layer.** What is added is a rule and one small helper.

- **No wrapper over rayon.** The parallel iterators are the abstraction; every
  call site is the same few lines (chunk into triples, map, collect). A wrapper
  would hide nothing and add a name to learn.
- **No executor trait, because a GPU is not "the same closure on a different
  executor".** A closure cannot be compiled to a shader; a per-pixel `?` with a pixel
  index becomes a flags buffer; data lives on the device across a whole *chain* of
  stages; readback is asynchronous; reductions need fixed-order trees. A GPU
  backend therefore plugs in at the orchestrator over the whole chain
  (reconstruction → transfer encode), uploading once and downloading or
  presenting once — the seams are the existing typed boundaries `FilmRgbImage`,
  `AcesCgImage` and `SharedDisplaySource`, not a new one per stage.
- **Kernel/driver split inside each stage.** Each per-pixel stage is a pure
  per-pixel function (`(triple, resolved params) -> triple`, e.g.
  `sdr::render_pixel_checked`) plus a buffer driver that applies it. The kernel is
  what a shader would reimplement and what it would be tested against; the driver
  is what rayon parallelizes. Most stages already have this shape. Where a loop
  fuses the map with a **reduction** — the MaxFALL sum in `hdr::render`, the gain
  min/max in `gain_map::build`, the clip counters in `quantize_u16` — the two must
  be separated anyway, because the map is order-free and a floating-point sum is
  not. That separation *is* the multithreading change in those places, not extra
  work. The GPU-only parts of the split (a uniform kernel signature, kernels as
  named public items, lookup tables arranged for textures, one kernel source for
  CPU and GPU) are **postponed** until a GPU backend is actually built.
- **Reductions stay explicit.** Floating-point sums run as their own sequential
  pass over the rendered buffer. Only integer counters, `min` and `max` may be
  folded in parallel.
- **One small helper module, justified by de-duplication only**: an in-place
  pixel map, a checked map that returns the first error with its pixel index, and
  an integer fold — one place for the chunk size and the `as_chunks` guard, and an
  API that cannot express a parallel f32 sum. If it ever grows a backend enum it
  has become the layer above.
- **Encoders get no abstraction.** libaom has row multithreading with a pinned
  thread count; the pure-Rust JPEG encoder has none; TIFF is I/O. They share
  nothing worth naming.
- **Thread count.** rayon's global pool; if control is ever needed it is an
  operational flag like `--max-memory` (arg struct only, never a recipe key) and
  must not touch libaom's pinned count, which changes bytes.

## Design notes for a future GPU path

- Move the **per-pixel** stages: reconstruction, ACEScg mapping, print controls,
  tone, gamut mapping, transfer encode. Keep the **reductions** on the CPU: film
  base, white-balance percentiles, MaxCLL/MaxFALL — they run once per image, not
  per knob.
- Settle first how the per-pixel math is written once and run on both CPU and GPU,
  so preview and final never drift. Candidates (`rust-gpu`, CubeCL) were **not**
  verified in this spike.
- Select the backend at the orchestrator over a whole stage chain (upload once,
  run kernels, download or present once), not per stage.
- The characteristic-curve reconstruction uses per-stock lookup tables; these
  become buffer/texture lookups.

## Reproduction

This reproduces the sequential `legacy` row of the timing table. The two
experiments were throwaway patches, reverted and not preserved: Experiment 1 was
rayon over the loops named under *Where the time goes today*; Experiment 2
changed only `g_threads` and `AV1E_SET_ROW_MT` in `io/avif.rs`. Their numbers are
re-measured when the multithreading work ships.

```sh
cargo build --release
BASE=$(./target/release/nc estimate ../nc-assets/rolls/2026-09-13-Portra400/base.tif \
  --base-region 2000,1400,800,600 | jq -r '.film_base | "\(.r),\(.g),\(.b)"')
./target/release/nc convert ../nc-assets/rolls/2026-09-13-Portra400/1675.tif \
  -o out.tif --output-preset legacy --film-base "$BASE" \
  --telemetry --telemetry-file tel.json
jq .timing_ms tel.json
```

Sources: [WGSL spec, floating point accuracy and reassociation](https://github.com/gpuweb/gpuweb/blob/main/wgsl/index.bs) ·
[Metal Shading Language Specification](https://developer.apple.com/metal/Metal-Shading-Language-Specification.pdf) ·
[wgpu `Limits`](https://docs.rs/wgpu/latest/wgpu/struct.Limits.html) ·
[wgpu-hal Metal `device.rs`](https://github.com/gfx-rs/wgpu/blob/trunk/wgpu-hal/src/metal/device.rs) ·
[wasm-bindgen-rayon](https://github.com/RReverser/wasm-bindgen-rayon) ·
[Using WebAssembly threads (web.dev)](https://web.dev/articles/webassembly-threads) ·
[GitHub runners and lavapipe](https://github.com/actions/runner-images/issues/2998)
