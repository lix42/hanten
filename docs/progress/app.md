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

**Status:** not started
**Updated:** 2026-10-15

- 2026-10-15: filed. Goal: an in-process export queue while sliders move: does the preview stay smooth, how do they share CPU and memory

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

## packaging-spike

**Status:** not started
**Updated:** 2026-10-15

- 2026-10-15: filed. Goal: sign and notarize a Tauri app with the Rust core; write the project file into any user-picked folder
