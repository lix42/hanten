# Hanten — app Progress Log

Execution log for the `app` epic: what was done and how, key decisions, what
works, what doesn't. Task status lives in each task file's frontmatter; this file is
the narrative beside it. Append entries (`tasks log <id> "…"`); don't rewrite earlier ones.

## Epic summary

- **Shell: Tauri on macOS** (`webkit-display-spike`, `docs/spike/webkit-display.md`).
  WKWebView shows a Display P3 2D canvas unclipped and equal to the export. HDR preview
  is a WebGPU canvas (`rgba16float`, `display-p3`, `toneMapping: extended`) fed linear
  Display P3. WebKit offers no float16 2D canvas, and gives less HDR headroom to a window
  that isn't focused.
- **The preview's latency is the render's.** IPC and draw add ~3 ms for SDR (7 MB RGBA8)
  and ~10 ms for HDR (29 MB f32; half-floats would halve it). At a 2048 edge the
  lcms2 display-curve encode costs as much as the chain (34 vs 28 ms), so the app's
  preview should apply the curve directly (37–54 ms render, 56–70 ms from slider to the next
  frame).
  Coalescing (one request in flight, newest value next) is enough for newest-wins.
- **Export beside preview** (`export-beside-preview-spike`,
  `docs/spike/export-beside-preview.md`):
  - In-process export works with a preview pool, an export pool, and export threads
    that pause between stages and pixel bands while the preview is active.
  - Never the shared global pool (p99 275 ms), and never a lowered QoS (7× slower on
    Apple silicon, with no preview running).
  - One export at a time; export decodes again.
  - lcms2 faults: one context per render job, each with its own handler.
- **Payment** (`offline-token-spike`): a proposal, not the final scheme — tokens bound
  to a device's P-256 key at download, the device sealing the paid record; see
  `docs/spike/offline-token.md`.

## webkit-display-spike

**Status:** done
**Updated:** 2026-10-15

- 2026-10-15: filed. Goal: Display P3 canvas, HDR preview and slider feel in WKWebView; decides Tauri vs Electron
- 2026-10-15: Started. Plan: real nc preview on a throwaway branch spike/webkit-display-core (lib target + preview module, never merged), Tauri 2 shell in ../temp/webkit-display-spike/shell, test page committed under docs/spike/webkit-display/. Checks: P3 canvas, HDR candidates, slider round trip with real renders, canvas vs export; Safari/Chrome for comparison.
- 2026-10-15: Done: report docs/spike/webkit-display.md, test page docs/spike/webkit-display/. Verdict: stay on Tauri. P3 canvas unclipped (readback + by eye on XDR); HDR via WebGPU extended canvas, gain-map/PQ images also HDR; slider-to-screen 96 ms p50 with lcms2 encode, 56 ms with the direct sRGB curve (IPC+draw ~3 ms); canvas readback equals the put bytes, factor-1 preview equals the export. Gotchas: WebKit cuts HDR headroom for an unfocused window (Safari too, Chrome not); no float16 2D canvas in WebKit; Chrome ignores toneMapping standard on WebGPU and loads no TIFF in img.
- 2026-10-15: Review fixes: the test page now probes each float16 2D option on a fresh canvas (WebKit still offers none), never draws an older frame than one drawn, waits on the GPU before timing the draw, and times both the next and the following animation frame. Re-ran in WKWebView: renders vary up to 1.5x between runs, so the report gives ranges (direct curve 56-70 ms to the next frame, 84 ms presented); sending every input was 1.5x slower than coalescing in run 2.
- 2026-10-15: Closed. Shipped review wording fixes: the sweep runs one input per animation frame (not a fixed 60/s), SDR draw time is only putImageData, HDR IPC ~7 ms. For packaging-spike: the shell pattern (plain Cargo crate, devUrl + remote.urls capability, ipc::Response for pixels) is in the report's How to rebuild.

## preview-cache-spike

**Status:** not started
**Updated:** 2026-10-15

- 2026-10-15: filed. Goal: a whole roll: cold load, one decode shared by measuring and preview, a memory budget, a disk cache

## export-beside-preview-spike

**Status:** done
**Updated:** 2026-10-16

- 2026-10-15: filed. Goal: an in-process export queue while sliders move: does the preview stay smooth, how do they share CPU and memory
- 2026-10-16: Started. Plan: throwaway core on branch spike/export-beside-preview-core (lib target + preview + in-process export, never merged) in ../temp/export-beside-preview-spike/core; synthetic 36-frame roll at 4927x3335 tiled from tests/fixtures/hdri-64bit.tif (the SSD with real scans is not mounted); headless harness for preview latency under export with scheduling variants (shared global rayon pool, separate pools, lower QoS for export, stage-boundary yield), concurrency 1/2/3, decode reuse; lcms2 per-context fault experiment; one Tauri shell sweep under export. Report docs/spike/export-beside-preview.md.
- 2026-10-16: Done: report docs/spike/export-beside-preview.md. Synthetic 36-frame roll (SSD not mounted); in-process export equals hanten convert in all u16 samples. Verdict: in-process export works with two rayon pools (preview, export) and a cooperative pause of export threads between stages and pixel bands while the preview is active (150 ms hold): drag p99 42 ms vs 40 idle, first render of a drag <=29 ms, export at full speed in rests; the shared global pool gives p90 161/p99 275. Lower QoS confines export to E-cores: utility 7x slower with nothing else running, background ~150x. One export at a time (~300 MB each, a second +16% throughput). Export decodes again (decode 11% of an export; a kept full decode is 263 MB). lcms2: global flag race 935/937 of 2000 renders wrong; one ThreadContext per render with its own handler 0/0; a context without a handler drops faults silently. WKWebView shell: WebKit frames unaffected by a full export (p99 20-24 ms). Gotchas: machine load 10-58 from other work, so passes were interleaved and medians reported; rayon install runs the job on pool threads, so per-thread marks go in the pool's start_handler. Handoff: design §2.7/§3.6 updated; preview-cache-spike gains the export memory and QoS facts.
- 2026-10-16: Review fixes: the lcms2 race's stolen faults are 935 of the clean thread's 4,000 renders (swallowed stays 937 of 2,000), not 935 of 2,000. The QoS slowdown was measured with no preview running, not on an idle machine (load 10-58), and the efficiency-core cause is inferred. The band gate's tail figures are the load 10-21 head-to-head; in the pass before (load 23-31) it reached p99 117 ms, no better than the stage gate. The shell's 36 ms frame-interval maximum holds for the export phases; one idle phase had a 113 ms warm-up frame.

## frame-offset-spike

**Status:** not started
**Updated:** 2026-10-15

- 2026-10-15: filed. Goal: is a frame's contrast or saturation offset a ratio or a step? settled by a review round

## offline-token-spike

**Status:** done
**Updated:** 2026-10-15

- 2026-10-15: filed. Goal: how tokens are signed, verified, stored and recovered on macOS
- 2026-10-15: Decided: the server binds each token to the device's P-256 public key at download; the device seals the paid record (token, sorted SHA-256 content hashes of the RGB u16 samples, replacements); a Keychain spent-token list stops a second spend. Bearer tokens cannot be bound offline: any self-made key seals any roll. Throwaway round trip in ../temp/offline-token-spike/ (not kept), 21 cases as designed. P-256 rather than Ed25519 because SecKey has no Ed25519 and the Secure Enclave is P-256 only. SHA-256 75 ms per 192 MB frame vs BLAKE3 90 ms single-threaded. Result: docs/spike/offline-token.md. Handoff: design §3.4/§3.7/§2.12 updated, §5 binding question removed; packaging-spike gains the data protection keychain question (needs a provisioning profile, TN3137).
- 2026-10-15: Review fixes: the ledger keeps each spent token's frame hashes, so copies of one record share one budget (cap + allowance; round trip: a second grown copy refused, 23 cases). Seal covers the hash scheme id; a scheme change keeps the old hashing path. Accepted limits now list ledger deletion, re-bind duplication on an offline old Mac and synced-Mac races; not settled adds key sync (packaging-spike verifies it) and adding frames on a new Mac without iCloud Keychain.
- 2026-10-15: Framing: the result is a spike proposal, not the final payment scheme. Design §3.7 says so and §5 keeps the binding question, now pointing to the note.
- 2026-10-15: Main finding (user): device binding confines an account's tokens to one Mac. Likely direction: spend online, or download one token just before spending it; adding frames to a paid roll on another Mac needs its own rule either way. Recorded at the top of the note and in design §5.
- 2026-10-15: Round 2 (user request): three machine bindings run from unsigned, ad-hoc signed binaries on macOS 26.7.1. Keychain key (file-based): seals, but a rebuilt binary can't read it (-25293, access list names the old binary). Data protection keychain: -34018 without entitlement. Secure Enclave key as a CryptoKit blob file: seals (Rust p256 verifies CryptoKit r||s), survives updates, never leaves the Mac, but any program of the user can sign with it; a damaged blob crashes the process inside CryptoKit; 21 ms per signature. Hardware UUID: stable, no secret, so it can't seal. None crosses Macs. Test Keychain items deleted. Results in the note's Round 2 section.

## packaging-spike

**Status:** not started
**Updated:** 2026-10-15

- 2026-10-15: filed. Goal: sign and notarize a Tauri app with the Rust core; write the project file into any user-picked folder
