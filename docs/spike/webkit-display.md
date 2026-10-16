# WebKit Display Spike

**Status:** closed. **Verdict: stay on Tauri.** WKWebView passed all four checks on
macOS: a Display P3 canvas, an HDR preview, the slider round trip, and a match with the
exported file. The render, not WebKit, sets the slider's latency.

**Investigated:** 2026-10-15 · task `app/webkit-display-spike` · base `c40951c` ·
release build, rustc 1.99.0 · Apple M3 Pro, built-in Liquid Retina XDR plus an external
SDR monitor (3440×1440) · macOS 26.6.2 · Tauri 2.12.4 (WKWebView), Safari 26.6.2,
Chrome 155 for comparison

## Question

Before any UI is built on Tauri ([desktop app design](../design/desktop-app.md) §3.1),
can WKWebView show what the app needs?

1. Does a Display P3 canvas show P3 colours unclipped?
2. Can an HDR preview be shown, and how?
3. How long from a slider move to the frame on screen, against the render time?
4. Does the canvas match the exported file?

## How it was tested

- **The test page**, [`webkit-display/index.html`](webkit-display/index.html), runs every
  check. Q1 and Q2 use synthetic patches and run in any browser. Q3 and Q4 need the shell.
- **Real previews.** The prototype's in-process preview
  ([native app prototype](native-app-prototype.md), "How it was built") was rebuilt on
  today's code. It lives on a throwaway branch, `spike/webkit-display-core`, which was never
  merged: a library target plus `nc::preview` (resolve, prepare, render). A factor-1
  preview again equals `hanten convert`'s Display P3 TIFF: all 49,294,635 samples are
  equal at 8 bits.
- **The Tauri shell** stays outside the repo with the code (`../temp/webkit-display-spike/`).
  - It has two commands: `open` (decode, then box-downsample to a 2048 edge) and
    `render(tweaks, generation, hdr, fast)`.
  - `render` returns a 28-byte header and the pixels as `ipc::Response`. The pixels are
    RGBA8 Display P3 for SDR, or RGBA f32 linear Display P3 for HDR, where 1.0 is
    reference white.
  - The core skips a request older than the newest it has seen.
- **The frame** is `2026-09-13-Portra400/1675.tif` (4927×3335), measured with its roll
  (`measure-roll --unexposed base.tif --leader leader.tif`). The preview is 1643×1112,
  factor 3.
- **Visual checks were the user's**, by eye on the XDR display. The numbers come from
  readback and the page's own timers.

## Findings

### Q1 — Display P3 canvas: yes

- A 2D canvas with `colorSpace: "display-p3"` stores P3 colours unclipped, through both
  `fillStyle` and `putImageData` (the app's path):
  - The P3 primaries read back as (255,0,0), (0,255,0) and (0,0,255).
  - The sRGB primaries read back as their P3 coordinates: (234,51,35), (117,251,76) and
    (0,0,245).
  - The same fills on a default (sRGB) canvas both read (234,51,35): clipped, as expected.
- By eye on the XDR, each P3 pair shows two different halves, as the CSS reference does. On
  the external SDR monitor, and on the sRGB canvas, the halves look the same.
- Safari and Chrome read back the same numbers.

### Q2 — HDR preview: yes, through a WebGPU canvas

| Candidate | WKWebView / Safari 26 | Chrome 155 |
|---|---|---|
| WebGPU canvas: `rgba16float`, `display-p3`, `toneMapping: {mode: "extended"}` | **HDR**: 2× and 4× visibly brighter than 1.0 | HDR |
| The same canvas with `toneMapping` `standard` (control) | clamped | **HDR as well**: the setting appears not to be honoured |
| 2D canvas, float16 (`colorType` with and without `toneMapping`, and `pixelFormat`, each on a fresh canvas) | not offered (all three ignored) | offered, but reports `toneMapping: standard` |
| `<img>` gain-map JPEG (Hanten's ISO 21496-1) | HDR | HDR |
| `<img>` PQ TIFF (BT.2020) | HDR | **TIFF not loaded** |

- The page also reports `(dynamic-range: high)` true and the CSS property
  `dynamic-range-limit` supported (applied as `no-limit`).
- **The HDR preview path.** The core sends linear Display P3, which the page uploads as an
  `rgba32float` texture and draws on an extended WebGPU canvas.
  - The shader applies the sRGB curve, extended past 1.0, since the canvas takes encoded
    values.
  - The user confirmed that diffuse tones match the SDR preview and the highlights are
    brighter.
- **WebKit limits headroom when the window isn't focused.** In both the Tauri app and
  Safari:
  - When the window isn't the active one, the 2× and 4× patches drop toward the 1.0 patch.
  - The gain-map and PQ images still look brighter than the SDR one, but less so.
  - Chrome keeps full headroom when unfocused.
  
  So this is WebKit's or the system's behaviour, not Tauri's. A user comparing the
  preview against another app sees less HDR in whichever window isn't focused.

### Q3 — Slider to frame on screen: the render dominates

Each setting was run as a scripted sweep on exposure for 3 s, one input per animation frame
(about 72/s on the external monitor, more on the XDR, so the input rate is not fixed across
runs). There
were two runs. Before run 2 the page was fixed: it never draws a frame older than one
already drawn, its HDR draw time waits for the GPU, and it also times the frame after the
change.
The SDR rows ran on the external monitor; the HDR row ran on the XDR in run 1 and the
external monitor in run 2.

| Setting | Render | IPC | Draw | Input → next frame (p50 / p90) | Input → presented, run 2 (p50 / p90) |
|---|---|---|---|---|---|
| SDR, lcms2 encode, coalesce | 73–74 ms | 2.4–2.6 ms | 1 ms | 96–97 / 103–112 ms | 111 / 118 ms |
| SDR, lcms2 encode, every input | 74–129 ms | 3.5–4.6 ms | 1 ms | 92–151 / 111–223 ms | 164 / 243 ms |
| SDR, direct sRGB curve, coalesce | 37–54 ms | 2.3–2.8 ms | 1 ms | 56–70 / 69–97 ms | 84 / 112 ms |
| HDR (f32 to WebGPU), coalesce | 35–40 ms | 6.5–7.1 ms | 3–6 ms | 67–70 / 70–81 ms | 83 / 94 ms |

SDR draw is only the `putImageData` call; its upload and composite fall into the frame times.
"Next frame" is the next animation frame after the draw, a lower bound. "Presented" is
the one after it, which starts once the changed frame is on screen, an upper bound. The
renders vary between runs by up to 1.5×, so read the ranges, not the digits.

- **WebKit's share is small.** IPC for the 7.3 MB RGBA8 frame takes 2–5 ms. The HDR frame
  in f32 is 29 MB and takes ~7 ms of IPC plus 3–6 ms of GPU draw; half-floats would halve
  the IPC.
- **The render is over the budget the design quoted.** At a 2048 edge, timed in the
  core's example outside the app, it splits into 6 ms decode, 28 ms chain and 34 ms for the
  lcms2 display-curve transform.
  - The chain alone matches the prototype's 24–35 ms.
  - The lcms2 transform is serial and costs as much as the chain.
  - Applying the sRGB curve directly in parallel (`pipeline::pixels`) gives 37 ms. Only
    195 of the 49M samples differ from lcms2, each by one 8-bit code.
  
  This is a task for the app's preview, not a WebKit limit (unknown 1).
- **Newest request wins.** Coalescing keeps one request in flight and sends the newest
  value when it returns.
  - Sending every input and letting the core drop stale requests (176–191 of 214–216
    skipped) was no better in run 1 and 1.5× slower in run 2.
  - No response arrived older than one already drawn (the guard counted none).
  - By hand, the user found every combination smooth.
  
  Use coalescing.

### Q4 — Canvas against the exported file: equal

- The canvas reads back exactly the bytes that were put, in `display-p3`: 0 of 5,481,048
  samples differ. With the factor-1 equality above, the canvas holds the export's pixels;
  readback cannot see compositing, which the next check covers by eye.
- By eye, the canvas beside `1675-sdr-p3.tif` in Preview matches in colour and tone.

## Unknowns this spike turned up

1. **The preview's display-curve encode costs as much as the chain.** Use the curve
   directly, or a parallel lcms2 transform, in the app's preview. That must stay within one
   8-bit code of the export (the factor-1 check), or the preview stops matching it.
2. **HDR headroom drops when the window isn't focused** (WebKit, not Tauri). Record it as
   a known behaviour; nothing in the app can change it.
3. **The HDR payload is 4× the SDR one.** Send half-floats, or the 16-bit encoded signal,
   if a 3000 edge or a slower Mac makes IPC matter.
4. **Electron (Chromium) differs from WebKit in ways that matter if Hanten ever switches:**
   - no TIFF in `<img>`;
   - `toneMapping` `standard` not honoured on a WebGPU canvas;
   - a float16 2D canvas, which WebKit lacks.
   
   Windows and Linux use different web views and need this check again (design §3.1).

## Not settled

- The 3000 edge and smaller Macs: timings are from an M3 Pro at a 2048 edge only.
- HDR on a non-XDR external HDR monitor; only the built-in XDR was tested.
- Packaging, signing and file access: `app/packaging-spike`.

## How to rebuild

- **Core.** Branch `spike/webkit-display-core` from `c40951c` (in
  `../temp/webkit-display-spike/core`, uncommitted, never merged):
  - `src/lib.rs` takes `main.rs`'s module list and adds `pub mod preview`.
  - `cli.rs` exposes `read_layer`'s body and the lcms2 handler as `pub(crate)` wrappers.
  - `src/preview.rs` has `resolve_recipe` (stamps `recipe_version: 3`), `prepare` (decode,
    `film_base::estimate`, then a box downsample with no IR plane) and `render_with`
    (`fixed::decode`, `map_nc_film_rgb_v1`, `chain::render` at SDR or HDR in Display P3,
    then the encode).
  - `examples/preview_check.rs` checks factor-1 equality against an export and times the
    renders.
- **Shell.** `../temp/webkit-display-spike/shell`, a plain Cargo crate (no `tauri-cli`)
  with its own `[workspace]`, depending on `nc` by path:
  - `devUrl` points at a static server over `www/`, where `www/page` links to this
    directory and `www/media` holds the exports.
  - The `remote.urls` capability allows that origin.
  - The `NC_SPIKE_SCAN`, `NC_SPIKE_ROLL` and `NC_SPIKE_RESULTS` environment variables
    fill the page's inputs. `NC_SPIKE_AUTO` makes the page run every sweep and save its
    results unattended.
