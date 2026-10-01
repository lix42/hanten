# Recipe replay fidelity

> **Re-scoped 2026-10-01** for the new chain. The original file was about recipes
> opting into a non-default curve (the 2026-08-03 sigmoid defaults, the 2026-09-09
> `density.scale` move) and the stopgap `cli::unpinned_curve`. All three are moot:
> recipes written before `pipeline_version` 8 are refused whole, and the stopgap is
> deleted. The four policy options and the two rejected remedies are in git. The
> problem below replaces them.

## Goal

Every document `hanten` writes can tell a later build that a default it relied on
moved. A replay either reproduces its render or says loudly that it cannot.

## The gap, measured 2026-10-01

`cli::pipeline_version_warning` is the only replay check. It reads
`meta.pipeline_version` from a `{meta, params}` envelope. Since `pipeline_version` 8
no run writes a sidecar, and every document a run does write is a **bare** recipe:

- `convert --dump-params` — no `meta`, no version;
- `measure-roll --out` (`roll.json`) — no `meta`, no version;
- `hanten params` — the same.

A hand-made envelope stamped 8 does warn on this build (9). Nothing hanten writes is
an envelope, so the check never fires in practice.

Two instances follow:

1. **The fallback slope (`pipeline_version` 9, `nf-calibration/no-roll-defaults`).** A
   `--dump-params` file written under 8 with no `roll.white_stops` replays under 9's
   moved fallback slope, with no warning. Values a recipe leaves unset come from the
   rendering's base (`crate::rendering`), which a default move changes.
2. **`roll.json` does not record the decode it measured under.** `measure-roll`
   carries `input`, `measure` and `reconstruction` into `--out` only when the input
   recipe stated them. Its gains, white and exposure are measured through the decode
   (`scale`, `linearization`, `anchor`), so a move to a decode default leaves every
   saved roll measurement wrong, without a warning.

## Design

Agreed direction (2026-10-01 review), to confirm at execution:

- **Stamp provenance** (`meta.pipeline_version`) into every document hanten writes,
  which brings the existing check back.
- **Always write the decode into `roll.json`**, so its measurements are pinned to the
  decode that produced them, stated or not.

Alternatives, if the direction does not hold:

- Write resolved values instead of nulls, so a document pins itself.
- A declared, tested table of moved defaults.

## Open questions

- Envelope or a new key? A bare recipe must not contain `meta` today
  (`split_envelope`). Switching `--dump-params` to the envelope changes the bytes
  `identity.params_hash` hashes.
- Does `hanten params` (a template, not a record) get stamped?
- What does the drift gate (`version::PIPELINE_FINGERPRINTS`) need to cover so the
  promise is enforced, not only stated?

## How to Verify

- A document written by each command, replayed after a simulated default move,
  warns (`--strict` promotes) or reproduces its render.
- A `roll.json` replays under its own decode even after a decode default moves.
- `docs/design-spec.md` §8 states the replay contract beside the identity/version
  contract, and `PIPELINE_VERSION`'s doc says what it does not cover.
- `docs/using-nc.md` updated for any change to a written document's shape.

## Dependencies

- [Conversion versioning & baseline comparison](conversion-versioning.md) — the label
  and the gate this extends.
- [Flip the default to the new flow](../nf-core/default-flip.md) — removed the
  sidecar, which is where the gap opened.
