# Offline Payment Tokens

How the desktop app's per-roll tokens are signed, verified, stored and recovered on macOS
(`docs/design/desktop-app.md` §2.11, §3.7). Produced by `app/offline-token-spike`,
2026-10-15. **A spike proposal, not the final payment scheme**: it shows one offline
binding that holds and what it costs, for the payment design to start from.

**Main finding:** binding a token to a device confines it to one Mac, unless the key
syncs through iCloud Keychain (a signed app, and only for users who turn it on).
Tokens downloaded to Mac A cannot be spent on Mac B without a server re-bind, which
duplicates them while A stays offline. A roll paid on A re-exports anywhere but grows only on A. The likely
direction is to **spend online** (the server signs the spend) or to **download one token
to a Mac just before spending it there**. Either fixes stranded tokens; adding frames to
a paid roll on another Mac still needs its own rule (online, or a server hand-over).
Round 2 ran three machine bindings for real; none crosses Macs.

**Investigated:** a throwaway Rust round trip in `../temp/offline-token-spike/` (not
kept) · rustc 1.99.0, release build · Apple M4 Pro · `p256` 0.14.0, `sha2` 0.11.1,
`blake3` 1.8.7. Storage claims come from Apple's documentation (TN3137, *Protecting keys
with the Secure Enclave*); no Keychain code was run.

## Question

1. Token format and signature scheme.
2. Where unspent tokens live on macOS, and how a reinstall recovers them.
3. What the token website must provide.
4. How the replacement allowance is recorded.
5. How a token is bound to one roll, when the server signs before the roll's hashes
   exist: a paid record copied to another project or hand-edited must still fail.

## The bar

The core's source is public (design §5), so a rebuilt core passes any check. "Light
protection" here means: **a text editor and a file copy cannot stretch a token past its
budget of files, and the shipped app refuses a second spend while its Keychain records
are intact.** It does not mean resisting a user who deletes those records, extracts their own
device key or patches the core (*Accepted limits*).

## Binding: the options

The server cannot sign the roll, so something it *did* sign must decide who may seal
the record. The only thing it can sign offline-compatibly is **the device's public key,
at download**.

| | copied record | edited record | 2nd spend, same Mac | token copied to another Mac | other Mac re-exports | other Mac adds frames | offline |
|---|---|---|---|---|---|---|---|
| bearer token, seal by a key in the core | refused (hashes) | **forgeable**: the key is in public source | needs a ledger | **not stopped** | yes | yes | yes |
| bearer token, seal by a device key the record names | refused | **forgeable**: any self-made key seals (round trip) | needs a ledger | **not stopped** | yes | yes | yes |
| bearer token, spent-token ledger outside the file | refused | refused only where the ledger lives | refused | **not stopped** | **refused**, or the check opens everything | no | yes |
| **device-bound token, device-sealed record** | refused | refused | refused (ledger) | refused | yes | with a synced key | yes |
| spend online | refused | refused | refused | refused | yes | online only | **no** |

A copied record is refused by every option, because its hashes are another roll's
frames. Editing the hashes is the attack, and only a seal the editor cannot reproduce
stops it. The device-bound token is the only offline option where that holds.

A copy of the **whole** project with the same negatives is the same roll and passes —
intended (save-as, a moved folder). Because a copy can also *grow*, the ledger keeps each
spent token's frames, not just its id: every copy of one record shares one budget of
cap + allowance files.

## Recommendation

### Token

Server-signed bytes, base64url, prefixed `hanten1.` (168 characters):

- key id (1 byte; the core holds a public key per id, so the server key can rotate),
- token id (16 random bytes),
- device public key (P-256, 33-byte compressed SEC1),
- frame cap (40) and replacement allowance,
- issue day.

The signature covers a domain tag plus those bytes: ECDSA P-256 with SHA-256 (the record
seal too), 64-byte `r‖s`.
**P-256, not Ed25519,** because the device key should be a Keychain *key* item.
`SecKey` has no Ed25519 algorithm, and the Secure Enclave handles only P-256. One curve
for both signatures keeps one verifier in the core.

### Paid record (project file)

- the token string,
- the content-hash scheme id,
- the sorted frame hashes,
- the replacements, as `old → new` pairs in order,
- the seal: the device's signature over a domain tag, token id, hash scheme id, frames
  and replacements.

Before every export the core checks, in order:

1. the token signature, with the built-in key for its key id;
2. the seal, with the token's device key;
3. the counts against the token's cap and allowance;
4. that every frame to export is in the record.

Adding a frame (up to the cap) or replacing one re-seals the record, which only the
paying device can do, and adds the frame to the token's ledger entry, which refuses it
past the shared budget. `SecKey` signs in X9.62 DER, which the core converts to `r‖s`.
Its RFC 4754 algorithm pads each half to 72 bytes on macOS 26 (`docs/spike/packaging.md`).

### Content hash

**SHA-256 over the RGB `u16` samples as read**, plus width and height: in
`io::decode`, the `DecodingResult::U16` buffer before normalisation. This leaves out:

- metadata, so a Lightroom or exiftool edit is not a "replacement";
- compression, so a rewritten file with the same pixels is the same frame;
- the IR plane.

On a 192 MB HDRi-sized buffer, SHA-256 takes 75 ms (2.6 GB/s, the CPU's SHA
instructions) against 90 ms for single-threaded BLAKE3. Both are small next to the
~750 ms cold decode (design §3.5), so the standard hash wins. The scheme id is there
because a decoder change that alters the samples would otherwise invalidate every paid
roll silently. Such a change must keep the old hashing path: migrating a record means
re-sealing it, which a roll paid on another or a lost Mac cannot.

### Replacement allowance

The allowance is signed into the token. The record lists each replacement, and the seal
covers the list, so clearing it breaks the seal (round trip). The number itself is
still design §5's question.

### Storage on macOS

- **Device key:** a P-256 key item in the **data protection keychain**, synchronizable.
  With iCloud Keychain on, a user's Macs share one device identity: any of them can add
  frames to a paid roll, and the ledger syncs with the key. That keychain syncs every
  item class from macOS 11, keys included, and Keychain Access shows only its *password*
  items, so the key isn't on view (TN3137; not run). A Secure
  Enclave key can never be extracted, but it can't sync either, so every Mac is
  separate. That's stronger than the bar needs and costs multi-Mac editing.
- **Spent-token ledger:** a synchronizable item in the same keychain, holding each spent
  token's frame hashes, and of tokens dropped after a re-bind or refund. It is the only
  thing stopping a second spend, and a user can delete it in Keychain Access (*Accepted
  limits*).
- **Unspent tokens:** not secret, since they're useless without the device key. A file
  in Application Support is enough, and the server can re-send them.
- **Entitlements:** the data protection keychain needs keychain access-group
  entitlements authorized by a provisioning profile, in an app-like bundle (TN3137). A
  bare `cargo run` binary can't use it, so development builds need a fallback that
  never ships: a key in a file behind a dev-only Cargo feature (`docs/spike/packaging.md`).
  The file-based keychain doesn't serve: it prompts after every rebuild.
- **Reinstall on the same Mac:** dragging an app to the Trash runs no code, so nothing
  removes its Keychain items; key, ledger and tokens survive. (Not run here.)
- **New Mac:** with iCloud Keychain, nothing to do. Without it, the user signs in and
  the server re-binds unspent tokens to the new key. Rolls paid on the old Mac still
  verify and re-export anywhere, because the key that checks them is inside the token,
  but only the old key can re-seal them: the new Mac cannot add frames or replace one
  (*Not settled*).

### The website

- A payment provider that handles sales tax.
- **Accounts** (email sign-in): binding and recovery both need one.
- **Issue:** the app signs in, sends its device public key and receives signed tokens.
  A token typed in by hand is useless, because it is bound to the device. A manual
  fallback would mean pasting the device key into the website.
- **Re-bind** for a new Mac, limited per account. The server can't know a token was
  spent offline, so the app reports spent token ids (the id only, no hashes) whenever it
  is online. The server refuses to re-bind a reported token, re-sends the unspent ones,
  and on the old Mac's next sign-in drops the tokens re-bound away from it. An old Mac
  that stays offline keeps them, so a re-bind duplicates its unspent tokens until then
  (*Accepted limits*).
- **Refunds** revoke unspent tokens on the server; a downloaded token stays valid
  offline (accepted).
- The signing key in a KMS or HSM. P-256 ECDSA is supported everywhere.

## What the round trip showed

23 cases, all as designed:

- **Allowed:**
  - a valid token verifies;
  - a 36-frame roll pays and re-exports, including a 10-frame subset;
  - frames can be added up to the cap of 40;
  - three replacements fit an allowance of three;
  - a second token pays a second roll;
  - one copy of a 1-frame record grows to 40 frames;
  - another Mac re-exports a paid roll.
- **Refused:**
  - a token whose cap was edited (token signature);
  - a 41st frame;
  - an unrecorded frame;
  - a record copied to another roll ("not paid for");
  - a record whose hashes were edited (seal);
  - a fourth replacement;
  - a record whose replacement list was cleared (seal);
  - a second copy of that record grown with other frames (79 files against a budget of
    43, ledger);
  - a second spend on the same Mac (ledger);
  - a token copied to another Mac;
  - a record sealed by the wrong Mac (seal).
- **Accepted limits:**
  - a user who extracts their own device key can seal a second roll with one token;
  - a token is 43 file slots, not one roll: a paid record copied to a new project and
    grown with another roll's files pays for it, so one token covers several short
    (120) rolls. Sealing the base frame's hash would bind the record to the roll;
  - a user who deletes the ledger and restores a token's string (from the project file
    or a backup of the tokens file) can spend it again;
  - after a re-bind, an old Mac that stays offline can still spend the re-bound tokens,
    and any it spent but never reported;
  - two Macs sharing a synced key, both offline, can each spend one token before the
    ledger syncs;
  - a bearer token lets any self-made key seal any roll. This is why the server must
    bind the key.

## Round 2: three machine bindings, run unsigned

What each way of tying a token to one Mac gives, run on macOS 26.7.1 from ad-hoc signed
binaries (no Developer ID, no provisioning profile; probes in
`../temp/offline-token-spike/round2/`, not kept). "Updated app" is a rebuilt binary with
a new code hash.

| | Keychain key (file-based) | Secure Enclave key (CryptoKit blob in a file) | Hardware UUID |
|---|---|---|---|
| seals a record | yes | yes: the Rust `p256` crate verifies CryptoKit's `r‖s` signature and rejects an edited message | **no**: nothing secret |
| updated app reads it | **no**: `-25293`; the item's access list names the old binary (a bare binary; a bundled app gets a dialog instead, `docs/spike/packaging.md`). A signed app's list names its team and bundle id (not run) | yes | yes |
| another program on the Mac | only if the user approves it in a password dialog, which also lets the user read the key out | **yes, silently**: any program of that user can sign with the blob, but it can never be copied off the Mac | yes |
| another Mac | no (a migrated keychain: not run) | never: only this Secure Enclave can use the blob (Apple) | never: a different id |
| app deleted, reinstalled | the item stays; an unsigned reinstall can't read it | yes, if the blob file stays | yes |
| macOS erased | lost | lost | **kept** |
| data protection keychain | `-34018`, missing entitlement | — | — |
| cost | — | 21 ms per signature, including process start; 284-byte blob | — |

- **A damaged blob crashes the process.** CryptoKit traps internally
  (`CryptoKitError.invalidParameter` under a `try!` in `SEP_P256.swift`) instead of
  throwing. The app would have to check the blob, or load it out of process.
- **The Secure Enclave key is the best device key without signing.** It seals records,
  survives updates and can't leave the Mac. But "another program can sign" means a short
  user-written program can seal records with it. That's a weaker lock than the accepted
  "extracted device key" limit: nothing has to be extracted.
- **The hardware UUID cannot replace a key.** It can only tell the server which Mac this
  is, for example so a re-bind after erasing macOS doesn't count against the limit.
- **None of the three works across a user's Macs.** That confirms the main finding.
  Only the synced data protection keychain would, and it needs a signed app.

## Not settled

- The replacement count per roll (design §5).
- Synced key or Secure Enclave key: this note recommends synced, because multi-Mac
  editing is worth more than non-extractability at this bar.
- The data protection keychain from the signed Tauri app, and whether a P-256 key item
  actually syncs (`app/signed-build-spike`). The development fallback is a key in a file,
  compiled out of release builds (`docs/spike/packaging.md`).
- Adding frames, on a new Mac without iCloud Keychain, to a roll the old Mac paid for. A
  server-signed hand-over (token id → new device key) that the record carries would let
  the new key re-seal it; its ledger starts empty, so the per-account re-bind limit is
  what bounds it.
- Windows and Linux storage.
- Whether the core goes private before release (design §5). Every option here assumes
  the shipped core.
