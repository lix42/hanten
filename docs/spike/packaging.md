# Packaging Spike

**Status:** closed. An ad-hoc signed Tauri app with the core inside runs under the
hardened runtime, exports in-process, and updates itself. Two parts moved to later tasks:

- File access on an external drive, a protected folder and a cloud folder
  (`app/file-access-spike`; [checklist](#file-access-checklist)).
- Signing, notarization and the synced device key, which wait on Apple Developer Program
  enrollment (`app/signed-build-spike`; [steps](#q6--signed-steps-not-run)).

**Investigated:** 2026-10-16 · task `app/packaging-spike` · core base `966c853` · release
build, rustc 1.99.1 · Apple M4 Pro · macOS 26.7.1 · Command Line Tools only, no Xcode, no
signing identity · Tauri 2.12.4, tauri-cli 2.12.3 (bundler 2.10.3), tauri-plugin-updater
2.14.0, tauri-plugin-dialog 2.8.3

## Question

1. Does the core, lcms2 included, run under the hardened runtime?
2. Updates: the updater's own signing, and what an update does to keychain items
   (folder access is Q3).
3. File access: scan folders, external drives, cloud folders.
4. Telemetry consent in the app.
5. A device-key fallback for unsigned development builds.
6. The signed steps, ready for `app/signed-build-spike`.

## How it was tested

- **Core.** A throwaway branch, `spike/packaging-core`, never merged. It adds a library
  target and `nc::app`: `probe` (a scan's header), `run` (any subcommand in-process,
  from an argv) and the CLI's allocator. The library leaves the allocator to its host
  binary, as the CLI's `main.rs` does, so the shell installs it.
- **Shell.** A plain Cargo crate outside the repo, `../temp/packaging-spike/shell`,
  bundled with `cargo tauri build`. It has commands to:
  - pick a folder and list and probe its scans;
  - write `roll.hanten.json` beside them (temp file, then rename);
  - export: `measure-roll --base-content` over the folder, then `convert --params` of
    the first frame;
  - check for and install an update;
  - probe a device key in each store.
- **Unattended runs.** `HANTEN_SPIKE_AUTO` lists steps, and the app writes one JSON line
  per step to `HANTEN_SPIKE_RESULTS`. Launched with `open --env`, so through
  LaunchServices, as Finder would launch it.
- **Bundle.** `signingIdentity: "-"` (ad-hoc), `hardenedRuntime: true`, no entitlements,
  `minimumSystemVersion` 14.0.

## Findings

### Q1 — Hardened runtime: the core runs, with no entitlements

- The bundle is signed `flags=0x10002(adhoc,runtime)` with no entitlements. `otool -L`
  lists only system frameworks: lcms2 is linked statically, so nothing else needs
  signing.
- In the installed app, two fixture frames were measured and one exported in 98 ms, and
  the TIFF is **byte-identical** to the CLI's `convert` with the same recipe.
- Sizes: the `.app` is 19 MiB (20 MB executable), the update archive 6.4 MiB.
- `spctl` rejects the ad-hoc bundle, as expected. Gatekeeper acceptance needs a
  notarized Developer ID build.

### Q2 — Updates: they work ad-hoc; keychain access does not survive them

- **Signing is Tauri's own**, a minisign key (`cargo tauri signer generate`), and is
  independent of Apple. `createUpdaterArtifacts` writes `.app.tar.gz` plus `.sig`, and
  the endpoint serves a static `latest.json`.
- **v0.1.0 updated itself to v0.2.0.** The bundle was swapped in place: new inode,
  v0.2.0, new CDHash, still `adhoc,runtime`. The app restarted into v0.2.0.
  - The swapped bundle carries no `com.apple.quarantine`, only `com.apple.provenance`.
  - The plugin swaps with `renamex_np(RENAME_SWAP)`. If the folder isn't writable it
    asks for an administrator password through AppleScript (source, not run).
- **A foreign signature is refused before install.** An archive signed by another key
  was rejected with "The signature was created with a different key than the one
  provided", and the bundle stayed v0.1.0.
- **Turn on `requireSignedVersion`.** `latest.json` itself is unsigned, so without this
  setting a crafted response can pair a higher version with an older, genuinely signed
  archive and force a downgrade. `cargo tauri build` already records `version:` in the
  signed comment, so it costs nothing. The default is `false`.
- **Plain http needs `dangerousInsecureTransportProtocol`.** It was used here for
  `127.0.0.1` only; the release endpoint must be https.
- **A file-keychain key does not survive an ad-hoc update without the user.**
  - v0.1.0 created a key in the login keychain. v0.2.0 then hit a SecurityAgent dialog
    to use it, because the key's access list names v0.1.0's ad-hoc signature (its
    CDHash).
  - After the user clicked **Always Allow**, it signed.
  - Every ad-hoc rebuild or update asks again. A Developer ID signature's requirement
    names the team and bundle id instead, so a signed update shouldn't ask (not run;
    `signed-build-spike`).

### Q3 — File access: moved to `app/file-access-spike`

Not run: the external drive wasn't mounted. Outside the sandbox the app reads
whatever its user can. The open questions are macOS's privacy prompts (TCC):

- Does a folder picked in the dialog stay readable after a relaunch and after an update?
  An ad-hoc signature changes with every build.
- Do removable volumes, Desktop/Documents and iCloud Drive each prompt?
- How does a cloud file that isn't downloaded yet behave?

The steps are in the [checklist](#file-access-checklist).

### Q4 — Telemetry consent: the app must own the helper and the record

- **The upload helper re-launches the app.** `telemetry::managed::spawn_helper` runs
  `current_exe() telemetry upload-once --generation …`, and in the app `current_exe()`
  is the app binary.
  - With consent on, an in-app export started exactly that process (seen, pid logged).
  - The shell's `main` catches a leading `telemetry` argument before Tauri starts and
    runs it headless through `nc::app::run`: the upload went through, and the queue was
    emptied.
  - Without that check, Tauri would ignore the arguments and open a second app instance
    (inferred from the code, not run).
  - The demo was kept local: consent lived in a spike config folder and
    `NC_TELEMETRY_ENDPOINT=file:<path>` caught the upload.
- **The consent record lives in `~/.config/nc`** (`$XDG_CONFIG_HOME/nc` first). That
  path is the CLI's, and an app would share it with any CLI on the Mac.
  - Since the CLI doesn't ship, the app should own the record, in its Application
    Support folder.
  - The app should ask in its own UI. `enable`'s TTY prompt and `--yes` are CLI forms.
- What the app's consent screen shows (the manifest, the queue) and whether the app keeps
  one helper process or uploads on a thread are UI decisions, not packaging ones.

### Q5 — Development device key: a key in a file, compiled out of release

| | Data protection keychain | File-based keychain | Key in a file (`dev-key` feature) |
|---|---|---|---|
| unsigned / ad-hoc bundle | `-34018` (missing entitlement), from the bundled app too | creates and signs | creates, signs, found again |
| after a rebuild or update | — | **asks the user** (SecurityAgent) | silent |
| in the release build | yes (the shipped path) | — | **absent**: `strings` finds its file name in the dev build (1), not in the release (0) |

The fallback is a P-256 key in a file in Application Support, behind a Cargo feature
that release builds never enable. The file keychain can't serve, because each dev build
would prompt. Dev keys should get tokens from a test signing key id that the release
core doesn't trust, so a dev-sealed record never verifies in a shipped app (not built).

**SecKey's RFC 4754 output is not 64 bytes.** On macOS 26.7.1,
`kSecKeyAlgorithmECDSASignatureMessageRFC4754SHA256` returns 144 bytes for P-256:

- `r` and `s` each sit right-aligned in a 72-byte field, and no error is raised.
- It is the same for a key that never touches a keychain and for a file-keychain key.
- Taken out of the padding, the signature verifies with `p256`.
- `…MessageX962SHA256` (DER, 70–71 bytes) verifies directly.

Sign with X9.62 and convert to `r‖s` in the core (`p256::ecdsa::Signature::from_der`).
The offline-token note had assumed RFC 4754 returns `r‖s`. The data protection
keychain's output is untested.

### Q6 — Signed steps (not run)

For `app/signed-build-spike`, from TN3137, Apple's keychain-sharing guide and Tauri's
signing docs:

1. **Certificate.** Enroll, create a *Developer ID Application* certificate and import
   it. `security find-identity -v -p codesigning` should then list it, and it goes in
   `bundle.macOS.signingIdentity` (or `APPLE_SIGNING_IDENTITY`).
2. **Provisioning profile.** Register the explicit App ID `<bundle id>`, then create a
   *Developer ID* provisioning profile for it. Embed it as
   `Contents/embedded.provisionprofile` through `bundle.macOS.files`. The data protection
   keychain needs its entitlements *authorized by a profile* in an app-like bundle
   (TN3137).
3. **Entitlements** (`bundle.macOS.entitlements`):
   - `com.apple.application-identifier` = `<TEAMID>.<bundle id>`, which is the default
     keychain access group;
   - `com.apple.developer.team-identifier` = `<TEAMID>`;
   - `keychain-access-groups` is optional.
   
   Xcode adds the first two; `codesign` from Tauri does not. **The bundle id is then
   permanent**: changing it orphans every user's device key.
4. **Notarize.** Set `APPLE_API_ISSUER`, `APPLE_API_KEY` and `APPLE_API_KEY_PATH` (or
   `APPLE_ID`, `APPLE_PASSWORD` and `APPLE_TEAM_ID`), then `cargo tauri build` notarizes
   and staples. The updater archive is made after that step.
5. **Verify:**
   - `spctl -a -vv` should give `source=Notarized Developer ID`, on a Mac that never
     built it.
   - The key probe in `data-protection` should give `created`, then `found` after an
     update.
   - The same probe on a second Mac with the same Apple ID and iCloud Keychain on should
     give `found`, with the same public key prefix.

## File access checklist

For `app/file-access-spike`. With the SSD mounted, rebuild (below), install v0.1.0, and
serve v0.2.0. Then:

1. **External drive.** Pick a roll folder on `/Volumes/square`, then List + probe →
   Write project file → Measure + export. Note any TCC prompt and the export time.
2. **Relaunch.** Quit, then "Reopen last (no dialog)". Does access hold?
3. **Update.** Install v0.2.0, then "Reopen last" again. Does access hold across an
   ad-hoc update?
4. **Protected folders.** Repeat 1–3 with a folder in `~/Documents` and one in
   `~/Desktop`.
5. **iCloud Drive.** A folder with a scan evicted ("Remove Download"). How long does
   the probe take, and what fault comes back if the download fails?
6. **Read-only folder** (`chmod a-w`). Write project file should return `permission-denied`.

Afterwards, if the file keychain was probed, run the `keydel:file-keychain` step
(`scripts/run.sh <results> keydel:file-keychain`) with the same build that made the
key. Then run `tccutil reset All app.hanten.spike` and delete
`~/Library/Application Support/app.hanten.spike`, which also holds the dev key.

## Not settled

- File access: the [checklist](#file-access-checklist) (`app/file-access-spike`).
- Gatekeeper, notarization, the synced key, and whether a Developer ID update keeps
  keychain and folder access (`app/signed-build-spike`).
- Telemetry in the app: what the consent screen shows, and whether uploads use a helper
  process or a thread (design §5).
- Distribution format (dmg or zip) and the update host. Neither was needed to answer
  the questions.

## How to rebuild

The code is in `../temp/packaging-spike/` on the M4 Pro. A portable copy, without build
outputs or the updater's private key, is in the user's Google Drive under
`temp/hanten/packaging-spike/`. Its README covers the core patch and the update server.
On another machine, make a new updater key pair and put its public half in
`plugins.updater.pubkey` before building v0.1.0 and v0.2.0. Without that, the update
step is refused ("different key").

- **Core.** `spike/packaging-core` from `966c853`, uncommitted (in the copy, a patch
  against that commit):
  - `src/lib.rs` takes `main.rs`'s module list plus `pub mod app`.
  - `cli.rs` splits `run` into `run_from(argv)` (parse errors returned) and `dispatch`.
  - Cargo finds `src/lib.rs` on its own; the manifest is unchanged.
- **Shell.** `../temp/packaging-spike/shell`, its own `[workspace]`, depending on `nc`
  by path:
  - `src/main.rs` holds the commands, the unattended mode and the helper check;
    `src/keychain.rs` the key probes, through raw Security.framework calls, because the
    `security-framework` crate can't set `kSecAttrSynchronizable` on a generated key.
  - `scripts/bundle.sh` bundles with the updater key; `scripts/run.sh` runs a plan
    through LaunchServices; `scripts/telemetry-demo.sh` runs the consent demo locally.
  - The update server is `python3 -m http.server 8765` over `server/`, which holds
    `latest.json` and the v0.2.0 archive.
