# Flip the default to the new flow

## Goal

`--new-flow` stops existing, because there is no longer a second flow to select.
The chain's defaults have already moved piecewise: each `nf-retire` task that removes
a default component (the sigmoid, the `shoulder` tone, the `Dmax` anchor) flips that
default in the same change and pays its own `pipeline_version` bump. This task is the
last of those, plus the flag's removal and the record of what the default now means.

## Design

- **The flag becomes a removed-flag error**, on the `--algorithm` precedent
  (`cli::reject_removed_flags`): a hidden arg emitting actionable guidance, no alias.
  nc is unreleased, so old recipes get a migration error rather than compatibility.
- **A `pipeline_version` bump with its own `PIPELINE_FINGERPRINTS` row.** Never edit a
  historical row in place — that makes one version label two behaviours. Scope the row
  carefully: the `render` row hashes `reconstruct_and_print`, whose `reconstruct` half
  *is* the decode being kept, while `base` and `recipe` have nothing to do with the
  print path. Retire the print half, not the row (design-update Part 2, Decisions).
- **A before/after report** under `docs/reports/`, measured rather than asserted, and
  `docs/using-nc.md` re-verified **by running the binary** — the guide's contract is
  that it is checked against `nc`, and every staleness sweep has found its own examples
  broken in ways no changelog mentioned.

**This supersedes the flip half of
[`algo/split-default-migration`](../algo/split-default-migration.md).** That task
carried the same obligations for a narrower move (`characteristic-generic` as the
no-flag default) and the same release gate; the gate itself moves to
[the neutrality gate](../nf-calibration/neutrality-gate.md), which owns the threshold
and the decide-either-way rule. Read it first — its "known vs unknown" section is
still accurate about what a default move owes.

## Open questions

- **Does the default destination move in the same bump?** It need not: the container
  default is untouched by the chain's piecewise flips, so `nf-destinations/default-destination`
  can land on its own schedule.
- **What the report's prose claims.** Flipping the default changes what every stage
  *does*; prose naming an operation is a claim about the run and must be derived from
  the resolved chain.
- **Is there anything the old flow could do that nothing new can yet?** Retiring a
  path is a list of abilities the new code may need — but the list has to be produced
  before the flip, not after.

## Scope and the gap list (2026-09-27, with the user)

**Scope.** Removing the flag leaves the current chain's render unreachable, and no other
task deletes it, so this task does: `merge`, the print/output halves of
`ResolvedConfig`, `OutputPreset`, the current chain's render and encode paths and their
report, sidecar and telemetry code, keeping what the new chain reuses. One PR,
separate commits. The default destination is the new chain's axis defaults (SDR
Display P3 16-bit TIFF, where the current default is `gain-map-hdr`);
`nf-destinations/default-destination` may still move it. The new fingerprint row's
`render` hashes the fixed decode only; where it should stop is
`nf-verification/fingerprints`'.

**What the current chain does that the new one does not, and each verdict.** Produced
before the flip, as the migration rule asks.

| Ability | Verdict |
|---|---|
| `gain-map-hdr`'s Ultra HDR v1 XMP beside ISO 21496-1 | Dropped. `--range hdr` writes the ISO-only per-channel map; a reader that knows only the legacy XMP shows the SDR base |
| `ultra-hdr-v1` (legacy dialect only) | Dropped, as above; it was never HDR on Apple |
| `compatibility` (sRGB SDR TIFF) | Gap until `nf-destinations/easy-destination-rows` adds an sRGB gamut |
| `--black-point` (one subtraction on film RGB) | Its surviving half is `--display-black`; the flare half was closed as not needed (`nf-scene-correction/flare-removal`) |
| `--linear-range` | Gap; `nf-scene-correction/levels-knob` decides a home or retirement |
| `--auto-wb` | Dropped for `hanten measure-roll` (already a `Never` row) |
| Sidecar, `params_hash`, the report's `recipe` echo and its ~20 current-chain sections | Gap; `nf-core/report-contract`. `--dump-params` is the round trip meanwhile |
| `--telemetry` / `--telemetry-file` | Refused until `nf-core/report-contract` |
| `inspect` / `estimate` fields that read the current chain's config; roll's per-frame consistency warnings | `nf-core/subcommands` |
| Loading a current-chain recipe or sidecar | Refused with a migration message; no converter (nc is unreleased) |
| `nctool` driving a build with `--output-preset` | A reference build still takes it; the new build takes the destination axes with no `--new-flow` — this task |

## How to Verify

- A bare `hanten convert` resolves the new chain; `--new-flow` exits 2 with a migration
  message naming what replaced it, and a recipe without `recipe_version` 2 (every
  current-chain sidecar) fails to load with a migration message.
- The current chain's render code is gone, not unreachable: no `allow(dead_code)` stands
  in for a deletion.
- The new fingerprint row exists and the gate is green; no historical row changed.
- `docs/using-nc.md` verified against the binary, and the CI gates pass.

The neutrality gate is **not** a check here: it holds the decode's values, not the
chain flip, and records no dependent (`nf-calibration/neutrality-gate`).

## Dependencies

- [A minimal end-to-end render](minimal-end-to-end.md)
- [Audit every knob against the new flow](knob-availability-audit.md)
- [Retire the sigmoid and `simple`](../nf-retire/sigmoid-and-simple.md)
- [Retire the `shoulder` and `none` tones](../nf-retire/display-tones.md)
- [Retire the `Dmax` anchor machinery](../nf-retire/dmax-machinery.md)
- [The gain-map destination](../nf-destinations/gain-map-destination.md) — the
  product's default output must exist on the new chain before it becomes the only one
- [`measure-roll` places the roll's white](../nf-calibration/roll-white-rule.md) — the
  last planned move of the default render (after the black point, which it depends on);
  flipping first would describe a default that is about to change
