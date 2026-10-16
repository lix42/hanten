---
title: Rename the fit-range and HDR misnomers (--display-tone-headroom, --display-black, linear_headroom)
status: done
deps: [core/value-domain-terminology]
summary: the highlight range gets its own public name; display black and the duplicated display peak
done: 2026-10-16
---
# Rename the fit-range and HDR misnomers

## Goal

- `--display-tone-headroom` / `fit_range.headroom_stops` name a retired stage and say
  "headroom", which now means the HDR headroom; the quantity is the **highlight range**.
- `--display-black` / `{"black"}` places the film base in display stops; it sets no
  black level.
- `hdr_linear_tiff.linear_headroom` duplicates `chain.fit_range.display_peak`.
- P8 (moved here from `core/rename-roll-report-misnomers`):
  `chain.fit_range.dark_end.{film_base_stops, frame_dark_stops}` are display stops
  below mid-grey (positive = darker), opposite in sign to `roll.dark_stops`; renamed,
  not flipped. The internal `chain::SharedParams.frame_dark_stops` holds scene stops
  under the same name.

## Design

Decided 2026-10-15 (with the user and the other rename sessions):

- `--highlight-range` / `fit_range.highlight_range_stops`; the report's
  `chain.fit_range.highlight_range_stops`.
- `--dark-base` / `dark_end {"base": S}`.
- `hdr_linear_tiff.peak_sample`, beside `reference_white_sample`.
- `film_base_below_mid_stops` / `frame_dark_below_mid_stops`.
- Public spellings only, through serde renames and clap `long =`. Every Rust
  identifier behind them is `core/internal-term-renames`, which drops the renames
  once the identifier matches.
- A retired key whose old default was `null` is stripped from a recipe; every other
  value, and any in a hand-written `--frames` override, is refused. No
  `pipeline_version` bump; the `recipe` fingerprint is refreshed in place.

## How to Verify

- Every old spelling is refused with a migration message naming the new one
  (`recipe::check_body`, `cli::reject_removed_flags`), tested through the binary.
- `docs/terminology.md`'s "Known public misnomers" loses the rows, and
  `docs/using-nc.md` is re-verified against the binary.
- CI gates green; `--save-recipe` output carries only the new names.
