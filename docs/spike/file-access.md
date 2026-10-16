# File Access Spike

**Status:** done. Outside the sandbox, a folder the user picks in the open panel stays
readable and writable with no privacy prompt, across relaunches and ad-hoc updates. Code
that opens a protected folder the user did not pick gets a prompt, and that grant is lost
on every ad-hoc update. A cloud file not yet downloaded is fetched whole by a header read,
and fails fast with `ETIMEDOUT` when offline.

**Investigated:** 2026-10-16 · task `app/file-access-spike` · the
[packaging spike](packaging.md)'s app, rebuilt (core base `966c853`) · release build,
rustc 1.99.1 · Apple M3 Pro · macOS 26.6.2, iCloud "Desktop & Documents" on · external
APFS SSD over PCI Express, reported as *fixed* media · Tauri 2.12.4, tauri-cli
2.12.3

## Question

1. Does a folder on an external drive, in `~/Documents` or `~/Desktop`, or in iCloud
   Drive prompt when picked, and is it readable and writable after?
2. Does access hold after a relaunch, and after an ad-hoc update, without the dialog?
3. A cloud scan not yet downloaded: how long does reading it take, and what fault comes
   back when the download fails?
4. Does a read-only folder give a named write fault the app can act on?
5. The export time for a real roll on the external drive.

## How it was tested

- The packaging spike's shell and core, rebuilt from the portable copy, ad-hoc signed with
  the hardened runtime, installed in `~/Applications` and launched through LaunchServices
  (`open`). A new updater key; v0.1.0 → v0.2.0 → v0.3.0, each a new CDHash.
- Added to the shell: per-step time, the raw errno and `io::ErrorKind` of a fault, a file's
  `SF_DATALESS` flag (data only in the cloud), a read-every-byte step, an export of every
  frame, and `pick` / `reopen` steps for unattended runs.
- Steps ran from scripts; the user answered the open panel and any privacy prompt, and
  reported what appeared; a step that blocks on a prompt shows it in its time (5–37 s
  instead of milliseconds).
- The roll: the 34 real frames of `2026-09-20-Portra400` (16.5 MP, HDRi 64-bit, 4.5 GB),
  cloned into a test folder on the SSD. Protected folders held two of those frames.
- Code, scripts and every run's JSON lines: `../temp/file-access-spike/` (`results-m3/`).

## Findings

### The open panel is the grant, and it lasts

| Folder | How it was reached | Prompt | Relaunch | Ad-hoc update |
|---|---|---|---|---|
| external SSD | picked | none | holds | holds |
| `~/Documents` (iCloud-synced) | picked | none | holds | holds |
| `~/Downloads` | refused at a prompt, then picked | none at the pick | holds | holds |
| `~/Desktop` (iCloud-synced) | opened by path, never picked | **yes** (Allow) | holds | **prompts again** |

- **A picked folder is open with no prompt**, and stays open: a later launch reads and
  writes it by its stored path, with no dialog, after a relaunch and after two ad-hoc
  updates. Picking a parent (`~/Documents`) covers its subfolders.
- **That grant is invisible.** Hanten Spike did not appear under Privacy & Security →
  Files & Folders after the pick; the user can't see or revoke it there.
- **A protected folder opened without the panel prompts** ("Desktop" folder). Allow adds a
  *Desktop Folder* toggle under Files & Folders. That toggle is tied to the ad-hoc
  signature: after the update the same folder prompted again. A Developer ID signature's
  requirement names the team instead, so a signed update should keep it
  (`app/signed-build-spike`).
- **A pick overrides a refusal.** With `~/Downloads` refused at the prompt, picking it
  opened it at once, and it stayed open after a relaunch and an update. The toggle stayed
  off.
- **The external SSD never prompted.** It reports itself as fixed media; a USB stick or SD
  card, which macOS classes as removable, was not tried.

So the app should reach every scan folder through the open panel (or a drop, untested)
and remember paths, never search the disk or open a remembered path the user did not
pick. Whether that grant survives a reboot, or how long it lasts, was not tested.

### Two faults, one `ErrorKind`

| Cause | errno | Rust `ErrorKind` |
|---|---|---|
| refused at the privacy prompt | `EPERM` (1), "Operation not permitted" | `PermissionDenied` |
| folder not writable (`chmod a-w`) | `EACCES` (13), "Permission denied" | `PermissionDenied` |
| cloud file, offline | `ETIMEDOUT` (60) | `TimedOut` |

The app's remedies differ (pick the folder; choose another folder; reconnect), so it must
branch on `raw_os_error()`, not on `ErrorKind`. In the read-only folder both the project
file's temp file and the export folder failed with `EACCES` before any rendering.

### A cloud file is downloaded whole by its first read

- **Online:** probing an evicted 144 MB scan (reading only its header) took 1.29 s and
  left the file fully downloaded (`dataless` cleared); the full read after it took 50 ms.
  A header-only probe saves nothing, and a roll that is all in the cloud needs about
  44 s just to list its 34 frames at that rate.
- **Offline:** the probe failed after 0.6 s and a full read after 0.44 s, both with
  `ETIMEDOUT`; the file stayed dataless. No dialog appeared, and nothing stalled.
- **The core misnames it.** The probe returned exit 3, `decode: … not a readable TIFF:
  Operation timed out (os error 60)`, so a failed download looks like a broken scan. That
  is `io/truncated-file-fault`'s case; errno 60 identifies it.
- The `SF_DATALESS` stat flag tells, before any read, which frames will download.

### Export of a real roll on the external drive

All 34 frames, in-process in the app: **41.0 s**. `measure-roll --base-content` over the
roll 10.6 s, then 0.69–1.23 s per frame (median 0.87 s), 16-bit Display P3 TIFF, no
faults. List and probe of the 34 frames took 35 ms.

### Packaging: the build must force a static lcms2

The first build died in dyld before `main`: `lcms2-sys` had found Homebrew's
`little-cms2` through pkg-config and linked it dynamically, and the hardened runtime's
library validation refused it ("mapping process and mapped file (non-platform) have
different Team IDs"). The packaging spike saw a static lcms2 only because that Mac had no
Homebrew copy. Set `LCMS2_STATIC=1` (or the crate's `static` feature) for the app build,
and check `otool -L` lists system libraries only.

The CLI built on this Mac links Homebrew's lcms2 2.19.1 too, so which lcms2 a local build
uses depends on what the machine has installed.

## Not settled

- Removable media (USB stick, SD card), network volumes, and a folder dropped on the
  window instead of picked.
- How long a pick lasts: across a reboot, a move of the folder, or a long gap.
- Whether a Developer ID update keeps a prompt-based grant (`app/signed-build-spike`).
- Google Drive's streamed folders (the prototype spike's case): only iCloud Drive was run.

## How to rebuild

Start from the packaging report's [How to rebuild](packaging.md#how-to-rebuild). The
changes here are in `../temp/file-access-spike/` on the M3 Pro:

- `shell/src/main.rs` and `dist/index.html`: the added steps above.
- `scripts/bundle.sh` sets `LCMS2_STATIC=1`; `scripts/run.sh` launches
  `~/Applications/Hanten Spike.app` and takes an absolute results path.
- To reset the privacy grants afterwards, the app must still be registered:
  `tccutil reset All app.hanten.spike` fails once the bundle is deleted.
