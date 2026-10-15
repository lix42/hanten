---
title: Rename the fit-range and HDR misnomers (--display-tone-headroom, --display-black, linear_headroom)
status: todo
deps: [core/value-domain-terminology]
summary: the highlight range gets its own public name; display black and the duplicated display peak
---
# Rename the fit-range and HDR misnomers

## Goal

- `--display-tone-headroom` / `fit_range.headroom_stops` name a retired stage and say
  "headroom", which now means the HDR headroom; the quantity is the **highlight range**
  (candidate `--highlight-range` / `highlight_range_stops`). The internal `headroom_*`
  identifiers move with it (or in `core/internal-term-renames`).
- `--display-black` / `{"black"}` places the film base in display stops; it sets no
  black level.
- `hdr_linear_tiff.linear_headroom` duplicates `chain.fit_range.display_peak`.

## Design

Open: final spellings; whether `linear_headroom` is dropped or kept as the HDR
headroom in stops.

## How to Verify

- Every old spelling is refused with a migration message naming the new one
  (`recipe::check_body`, `cli::reject_removed_flags`), tested through the binary.
- `docs/terminology.md`'s "Known public misnomers" loses the rows, and
  `docs/using-nc.md` is re-verified against the binary.
- CI gates green; `--save-recipe` output carries only the new names.
