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
- **Packaging** (`packaging-spike`, `docs/spike/packaging.md`; design §3.8). An ad-hoc
  Tauri bundle with the core in-process runs under the hardened runtime with no
  entitlements. The Tauri updater works, and `requireSignedVersion` should be on.
  Gotchas:
  - `SecKey`'s RFC 4754 output is padded, so sign X9.62 DER.
  - The telemetry helper re-launches the app binary.
  - A file-keychain key prompts after every ad-hoc update.
  - lcms2 must be forced static (`LCMS2_STATIC=1`): library validation refuses a
    Homebrew copy, and the app dies in dyld.

  Signing and the synced key are `signed-build-spike` (needs enrollment).
- **File access** (`file-access-spike`, `docs/spike/file-access.md`):
  - A folder picked in the open panel opens with no prompt, protected or not, and stays
    open across relaunches and ad-hoc updates. The grant isn't shown in Settings.
  - A protected folder opened by path, never picked, prompts. An ad-hoc update loses
    that grant, so open only folders the user picked.
  - Privacy refusal is `EPERM`, an unwritable folder `EACCES`, an offline iCloud file
    `ETIMEDOUT`. Branch on the errno: Rust gives the first two the same `ErrorKind`.
  - An iCloud file is downloaded whole by its first read (~1.3 s per frame).
  - A 34-frame roll on the external SSD exports in 41 s.
- **Frame offsets** (`frame-offset-spike`, `docs/spike/frame-offset.md`; design §2.3).
  - Exposure is a step in EV.
  - Contrast, saturation and both colour controls are ratios, per channel for colour.
  - Tier-3 switches and named steps are plain overrides.
  - Lift keeps the picture of every other frame that overrides the knob.
  - Open: dragging over a mixed selection.
- **Preview cache** (`preview-cache-spike`, `docs/spike/preview-cache.md`; design §3.5):
  - The cache point is the frame's downsampled scan before the film base, f16 at a
    2048 edge (11 MB a frame; on the SDR preview ≤2/255 from f32).
  - A roll opens with the current frame alone, then 2 prepare workers: first preview
    in 0.15–0.4 s, 34 frames in 2.3–3.2 s.
  - An unmeasured roll decodes each frame once for its preview and its measurement,
    unless an `auto` base or leader, or a content base, needs a first pass.
  - A disk cache reopens a roll in ~0.1 s.
  - Gotchas:
    - Measuring one frame at a time keeps ~1.2 cores busy. Run frames on plain threads
      and merge them in input order; a confined rayon pool confines their nested loops
      too, and made it slower.
    - Every full decode in flight costs up to ~265 MB on top of the cache.

## webkit-display-spike

**Status:** done
**Updated:** 2026-10-15

- 2026-10-15: filed. Goal: Display P3 canvas, HDR preview and slider feel in WKWebView; decides Tauri vs Electron
- 2026-10-15: Started. Plan: real nc preview on a throwaway branch spike/webkit-display-core (lib target + preview module, never merged), Tauri 2 shell in ../temp/webkit-display-spike/shell, test page committed under docs/spike/webkit-display/. Checks: P3 canvas, HDR candidates, slider round trip with real renders, canvas vs export; Safari/Chrome for comparison.
- 2026-10-15: Done: report docs/spike/webkit-display.md, test page docs/spike/webkit-display/. Verdict: stay on Tauri. P3 canvas unclipped (readback + by eye on XDR); HDR via WebGPU extended canvas, gain-map/PQ images also HDR; slider-to-screen 96 ms p50 with lcms2 encode, 56 ms with the direct sRGB curve (IPC+draw ~3 ms); canvas readback equals the put bytes, factor-1 preview equals the export. Gotchas: WebKit cuts HDR headroom for an unfocused window (Safari too, Chrome not); no float16 2D canvas in WebKit; Chrome ignores toneMapping standard on WebGPU and loads no TIFF in img.
- 2026-10-15: Review fixes: the test page now probes each float16 2D option on a fresh canvas (WebKit still offers none), never draws an older frame than one drawn, waits on the GPU before timing the draw, and times both the next and the following animation frame. Re-ran in WKWebView: renders vary up to 1.5x between runs, so the report gives ranges (direct curve 56-70 ms to the next frame, 84 ms presented); sending every input was 1.5x slower than coalescing in run 2.
- 2026-10-15: Closed. Shipped review wording fixes: the sweep runs one input per animation frame (not a fixed 60/s), SDR draw time is only putImageData, HDR IPC ~7 ms. For packaging-spike: the shell pattern (plain Cargo crate, devUrl + remote.urls capability, ipc::Response for pixels) is in the report's How to rebuild.

## preview-cache-spike

**Status:** done
**Updated:** 2026-10-16

- 2026-10-15: filed. Goal: a whole roll: cold load, one decode shared by measuring and preview, a memory budget, a disk cache
- 2026-10-16: Started. Plan: throwaway core on branch spike/preview-cache-core (lib target, preview prepare/render, in-process per-frame measure from a shared decode; never merged) in ../temp/preview-cache-spike/core; real roll rolls/2026-09-20-Portra400 (36 frames + base + leader) on the M3 Pro 18 GB. Harness: decode-twice vs shared decode, measured vs unmeasured roll, 1-4 prepares at once; time to first preview / all previews / measured roll, peak footprint; warm and cold (SSD after sudo purge; cloud not run). Memory: footprint vs frames cached at 2048/3000 edge, f32 vs f16, one open beside an export. Disk cache of downsampled scans: reopen time, size, staleness keys. Report docs/spike/preview-cache.md.
- 2026-10-16: Done: report docs/spike/preview-cache.md, on the real roll 2026-09-20-Portra400 (34 pictures + base + leader) on the M3 Pro 18 GB, load 4-50 so variants interleaved and medians reported. Measured roll, 2048 f32: first preview 0.16-0.4 s, all frames 2.3-3.2 s at 2 workers; more workers cost ~150-270 MB each for little time. Cold reads (after sudo purge) from the external PCIe SSD add ~35 ms/frame (44 vs 10 ms), inside the noise; a cloud folder not run. Shared decode: hook in decode_for_roll_white downsamples each measuring decode, roll.json equal to the CLI's, costs measuring nothing; saves the second read/decode and 0.4 GB peak vs decoding twice. measure-roll's picture loop on plain threads (merged in input order, roll.json equal): 8.8 s -> 6.6 s (2) -> 5.1 s (3); a confined 2-thread rayon pool was slower (11.2 s) because it confines nested par loops. f16 cache: 11 MB/frame, 1.8% of 8-bit samples differ by <=2. Disk cache f16: whole roll back in 112-125 ms cold from the internal drive, 376 MB. Opening beside an ungated export: peak 1.48-1.55 GB, first preview up to 2.35 s. Handoff: design 3.5 rewritten, 5 open questions replaced (prepare priority beside export, current frame before leader, session memory model, disk cache cap).
- 2026-10-16: Review fixes: the shared decode saves 0.36 GB of peak against decoding twice only with one measuring thread, ~0.1 GB with two (the Done entry's 0.4 GB); it does not speed measuring by itself (8.3 s on one thread, as decoding twice cold), the 6.4 s comes from the second measuring thread. A measured roll opens in 2.3-3.2 s at 2 workers (not 2-3 s). offline-token.md's '~750 ms cold decode (design 3.5)' pointed at text 3.5 no longer has and was a cloud read, not a decode; now the decode's 94-230 ms from preview-cache.md.
- 2026-10-16: Closed. Landed: report docs/spike/preview-cache.md; design 3.5 rewritten (f16 downsampled scan as the cache point, 2 prepare workers, one decode per frame for preview and measurement pooled in input order, disk cache keyed by file identity); design 5 lists what stays open. Verified: in-process, shared-decode and threaded measure-roll each wrote a roll.json equal to the CLI's. For the app epic: prepares beside an export need a priority, and pipeline/memory.rs needs a session model (cache + decodes in flight + export).
- 2026-10-16: PR review fixes (#319): one decode per frame holds only with named base and leader frames; --unexposed/--leader auto and --base-content read every frame in a first pass (the cache can fill from it). A crop needs its own downsample entry keyed by the crop: cutting the whole frame's 2048-edge downsample halves a half frame's resolution. Disk-cache key carries a decode/downsample/format version (design 3.5 had dropped it). f16 verdict scoped to the SDR preview; HDR listed open. Open the current frame alone, then 2 workers (a second worker slowed the warm first preview 158 -> 391 ms). offline-token.md compares hashing with exporting a frame (0.4-1.2 s), not the decode.
- 2026-10-16: Review of those fixes: one decode per frame fails only with an auto base or leader or a content base (a named base without a leader, or a stated base, needs no first pass); the cache entry is keyed by the crop as well, so 'never by a knob' became 'never by a rendering knob'; a half frame cut from the whole downsample keeps under half the preview's pixels (the previous entry's 'halves its resolution' was the wrong measure: factor 3 vs 2, ~0.44 of the pixels); the current-frame-alone scheme is inferred from the 1- and 2-worker runs, not run.

## export-beside-preview-spike

**Status:** done
**Updated:** 2026-10-16

- 2026-10-15: filed. Goal: an in-process export queue while sliders move: does the preview stay smooth, how do they share CPU and memory
- 2026-10-16: Started. Plan: throwaway core on branch spike/export-beside-preview-core (lib target + preview + in-process export, never merged) in ../temp/export-beside-preview-spike/core; synthetic 36-frame roll at 4927x3335 tiled from tests/fixtures/hdri-64bit.tif (the SSD with real scans is not mounted); headless harness for preview latency under export with scheduling variants (shared global rayon pool, separate pools, lower QoS for export, stage-boundary yield), concurrency 1/2/3, decode reuse; lcms2 per-context fault experiment; one Tauri shell sweep under export. Report docs/spike/export-beside-preview.md.
- 2026-10-16: Done: report docs/spike/export-beside-preview.md. Synthetic 36-frame roll (SSD not mounted); in-process export equals hanten convert in all u16 samples. Verdict: in-process export works with two rayon pools (preview, export) and a cooperative pause of export threads between stages and pixel bands while the preview is active (150 ms hold): drag p99 42 ms vs 40 idle, first render of a drag <=29 ms, export at full speed in rests; the shared global pool gives p90 161/p99 275. Lower QoS confines export to E-cores: utility 7x slower with nothing else running, background ~150x. One export at a time (~300 MB each, a second +16% throughput). Export decodes again (decode 11% of an export; a kept full decode is 263 MB). lcms2: global flag race 935/937 of 2000 renders wrong; one ThreadContext per render with its own handler 0/0; a context without a handler drops faults silently. WKWebView shell: WebKit frames unaffected by a full export (p99 20-24 ms). Gotchas: machine load 10-58 from other work, so passes were interleaved and medians reported; rayon install runs the job on pool threads, so per-thread marks go in the pool's start_handler. Handoff: design §2.7/§3.6 updated; preview-cache-spike gains the export memory and QoS facts.
- 2026-10-16: Review fixes: the lcms2 race's stolen faults are 935 of the clean thread's 4,000 renders (swallowed stays 937 of 2,000), not 935 of 2,000. The QoS slowdown was measured with no preview running, not on an idle machine (load 10-58), and the efficiency-core cause is inferred. The band gate's tail figures are the load 10-21 head-to-head; in the pass before (load 23-31) it reached p99 117 ms, no better than the stage gate. The shell's 36 ms frame-interval maximum holds for the export phases; one idle phase had a 113 ms warm-up frame.

## frame-offset-spike

**Status:** done
**Updated:** 2026-10-16

- 2026-10-15: filed. Goal: is a frame's contrast or saturation offset a ratio or a step? settled by a review round
- 2026-10-16: Started. Plan: analysis note docs/spike/frame-offset.md (offset meaning per knob kind, ratio vs step algebra, lift keeps other frames' look); real roll with placed and thin frames, base slopes read from reports; targeted 4-arm review (contrast and saturation, ratio vs step at the points they differ most); decision into design §2.3, §5.
- 2026-10-16: Contrast review (user, 8 frames of 09-29 Ektar, hybrid): roll 1.0 -> 1.5 with a frame tweaked to 0.7; the ratio rule's 1.05 against roll 1.5 looks like the tweak against roll 1.0, the step rule's 1.2 does not. Set ../temp/frame-offset-spike/contrast/.
- 2026-10-16: Saturation review (user, same 8 frames): roll 1.0 -> 1.5 with a frame at 0.6; ratio 0.9 preferred over step 1.1, but hard to tell (weak). Decision: both are ratios, on the review and the algebra (a step leaves the positive range, a ratio composes with the knob's own ratio on its base). Set ../temp/frame-offset-spike/saturation/.
- 2026-10-16: Done: note docs/spike/frame-offset.md, design §2.3/§3.4/§5 updated. Exposure a step in EV; contrast, saturation and both colour controls ratios (per channel for colour); tier-3 switches and named steps plain overrides that pin the frame. An identity offset is no override. Lift: roll takes the source's value, source clears, other frames overriding the knob keep their picture (offset restated), frames without one follow; unavailable over a mixed selection. A roll grade change pushing a frame past the spread bound is refused, naming the frame. UI: frame value with the roll's beside it, offset in the knob's unit, log-scale sliders for ratios. Review fixes: lift wording aligned across note, design and epic summary; the grade's reason no longer claims exponents compose (the grade rescales luminance). Open (design §5): dragging over a mixed selection.

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

**Status:** done
**Updated:** 2026-10-16

- 2026-10-15: filed. Goal: sign and notarize a Tauri app with the Rust core; write the project file into any user-picked folder
- 2026-10-16: Started. Split (user): signing, notarization and the synced data protection keychain key moved to app/signed-build-spike, blocked on Apple Developer Program enrollment (this Mac has no code-signing identity, Command Line Tools only). Plan here: throwaway core spike/packaging-core (lib target, open/measure/export), Tauri 2 shell in ../temp/packaging-spike/shell, ad-hoc signed .app with the hardened runtime, updater v1->v2 on a local server, dev keychain fallback, telemetry consent answer; report docs/spike/packaging.md. File access (external drive, real roll, project file, export) deferred to another machine: the SSD isn't mounted here.
- 2026-10-16: Unsigned half run on the M4 Pro (macOS 26.7.1), report docs/spike/packaging.md. Ad-hoc bundle (adhoc,runtime, no entitlements, lcms2 static) exports in-process, byte-identical to the CLI. Updater v0.1.0->v0.2.0 swaps the bundle in place, no quarantine xattr; a foreign signature is refused; set requireSignedVersion (the CLI already signs the version). Gotchas: a file-keychain key prompts (SecurityAgent) after every ad-hoc update; SecKey RFC 4754 returns 144 bytes for P-256 (r and s right-aligned in 72-byte halves), so sign X9.62 DER and convert; the telemetry helper re-launches current_exe(), i.e. the app, so main must route a leading 'telemetry' arg headless before Tauri; the consent record sits in ~/.config/nc, shared with the CLI. Dev fallback: P-256 key in a file behind a dev-key feature, absent from the release binary (strings). Still to run on another machine: external drive, Documents/Desktop, iCloud Drive, read-only folder, TCC grants across relaunch and ad-hoc update (checklist in the report). offline-token.md's RFC 4754 sentence corrected.
- 2026-10-16: Closed (user): file access split into app/file-access-spike (checklist in the report), signing into app/signed-build-spike. Handoff: design §3.8 Packaging and updates added, plus §2.12 enrollment gate, §3.2 allocator and current_exe() relaunch, §3.6 in-process export result, §5 open questions; report status closed. Portable copy of the spike code (core as a patch against 966c853, shell, scripts, results; no build outputs, no updater private key, which the auto-mode check refused to upload) in the user's Google Drive, temp/hanten/packaging-spike.

## signed-build-spike

**Status:** not started
**Updated:** 2026-10-16

- 2026-10-16: filed. Goal: notarize the Tauri app and run a synced P-256 key in the data protection keychain across two Macs; waits on Apple Developer Program enrollment

## file-access-spike

**Status:** done
**Updated:** 2026-10-16

- 2026-10-16: filed. Goal: the packaged app on scan folders: external drive, Documents/Desktop, iCloud Drive, a read-only folder; do access grants survive a relaunch and an update
- 2026-10-16: Started on the M3 Pro (macOS 26.6.2, SSD mounted). Plan: rebuild the packaging spike from the Drive copy into ../temp/file-access-spike (core patch on throwaway spike/file-access-core from 966c853, new updater key pair, v0.1.0 + v0.2.0); shell gains step timing, errno capture and a full-read step; run the checklist live: real roll on /Volumes/square, relaunch, ad-hoc update, synced Documents/Desktop (iCloud Desktop & Documents is on here) plus ~/Downloads as a local protected folder, an evicted iCloud scan (Wi-Fi off for the failure), a read-only folder. Report docs/spike/file-access.md.
- 2026-10-16: Checklist run on the M3 Pro, report docs/spike/file-access.md. First build died in dyld: lcms2-sys linked Homebrew's lcms2 via pkg-config and library validation refused it; LCMS2_STATIC=1 fixes it (packaging.md's static claim held only on a Mac without Homebrew lcms2; the CLI here links Homebrew's 2.19.1 too). Results: a picked folder (SSD, ~/Documents, ~/Downloads after a refusal) opens with no prompt and holds across relaunch and two ad-hoc updates, with no Files & Folders entry; ~/Desktop opened by path prompted, and prompted again after the ad-hoc update. Faults: privacy refusal EPERM (1), chmod a-w EACCES (13), both PermissionDenied in Rust; offline evicted iCloud file ETIMEDOUT (60) in 0.4-0.6 s, which the core reports as exit 3 'not a readable TIFF'. Online, a header probe of an evicted 144 MB scan downloads it whole in 1.29 s. Export of 34 real frames (2026-09-20-Portra400) on the SSD: 41.0 s (measure 10.6 s, 0.87 s median per frame). Handoff: design §2.8/§3.8/§5, signed-build-spike Q3 narrowed to prompt-based grants, io/truncated-file-fault gains the iCloud ETIMEDOUT case. Test folders, app, app data and TCC grants removed; code and results in ../temp/file-access-spike.
- 2026-10-16: Done. Review (Codex: clean; diff reviewer: §3.8 said 'updates' where only ad-hoc ones ran, and two open-question lines still listed folder access in general) fixed: design §3.8 now says ad-hoc updates, the signed-build open questions name only a prompt-based grant, design §5 adds Google Drive streamed folders. For signed-build-spike: compare a Files & Folders toggle across a Developer ID update; picked folders need no check. For io/truncated-file-fault: errno 60 marks an offline iCloud read. Not filed (raised with the user): a local CLI build links Homebrew's lcms2 when present.
