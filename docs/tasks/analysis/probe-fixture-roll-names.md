# Re-key the asset probes to today's roll names

## Goal

Make the `#[ignore]`d asset probes (`pipeline::shadow_metrics`, `algo::curve_probe`)
run again against `../nc-assets`. Their `FIXTURES` tables look rolls up in
`manifest.json` by names the asset folder no longer uses (`Ektar`, `Portra160`,
`Portra400`, …); the folder now names rolls by date (`2026-07-15-Ektar100`), so
those probes panic before measuring anything. `2026-07-24-Gold200` is already
current.

This matters because the probes are the evidence several docs cite, and moving a
default is supposed to be followed by re-running them — which is impossible today.

## Known

- `cargo test` never runs them, so every gate is green while they are broken.
- Each entry pairs a manifest roll key with a frozen recipe stem under
  `scripts/real-scan-verify/recipes/`. Dated recipes exist for some rolls
  (`2026-09-09-Ektar`, `2026-09-11-Portra400`) but not all.

## Open questions

- Which dated roll corresponds to each old name, and is it the *same frames* the
  recorded measurements came from? A re-key onto different frames changes the
  numbers the docs quote.
- Should the recipe stems move to the dated recipes too, or stay frozen?
- Should a missing roll skip with a message, as a missing asset folder does,
  rather than panic?

## How to verify

`cargo test --release -- --ignored` with assets present runs every probe to
completion; the figures the docs cite either reproduce or are updated with a note.

## Dependencies

- `analysis/asset-manifest`
