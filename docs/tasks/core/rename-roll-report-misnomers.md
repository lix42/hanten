---
title: Rename the roll and report misnomers (frames.*.exposure, slope_bound, dark_stops sign, area tilt, neutral_balance, saturation_margin)
status: todo
deps: [core/value-domain-terminology]
summary: roll keys and report fields whose names collide, mislead or flip sign, plus American spellings
---
# Rename the roll and report misnomers

## Goal

From `docs/terminology.md`'s "Known public misnomers":

- `roll.frames.*.exposure` is the small lift (`roll.frame_exposure`), not exposure.
- `slope_bound` is an enum in `frames[].hybrid` and a number in `thin_lift`.
- `roll.dark_stops` (negative = darker) vs `frame_dark_stops` (positive = darker).
- `area_uniformity.tilt` collides with the white-balance form `tilt`.
- `--neutral-balance` / `roll.neutral_balance` switches the roll colour correction.
- `saturation_margin_stops` means film saturation, beside `look.saturation`.
- American spellings in the surface: `input_color`, `gray-world`.

## Design

Open: which of these are worth a migration before the version reset, and whether
the sign fix flips a field or renames it.

## How to Verify

- Every old spelling is refused with a migration message naming the new one
  (`recipe::check_body`, `cli::reject_removed_flags`), tested through the binary.
- `docs/terminology.md`'s "Known public misnomers" loses the rows, and
  `docs/using-nc.md` is re-verified against the binary.
- CI gates green; `--save-recipe` output carries only the new names.
