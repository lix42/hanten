---
title: Rename the roll and report misnomers (frames.*.exposure, neutral_balance, slope_bound, area tilt, saturation_margin)
status: done
deps: [core/value-domain-terminology]
summary: roll keys and report fields whose names collide or mislead
done: 2026-10-16
---
# Rename the roll and report misnomers

## Goal

From `docs/terminology.md`'s "Known public misnomers":

- `roll.frames.*.exposure` is the small lift (`roll.frame_exposure`), not exposure.
- `--neutral-balance` / `roll.neutral_balance` switches the roll colour correction.
- `slope_bound` is an enum in `frames[].hybrid` and a number in `thin_lift`.
- `area_uniformity.tilt` collides with the white-balance form `tilt`.
- `saturation_margin_stops` means film saturation, beside `look.saturation`.

Moved or dropped (2026-10-15, with the user): the `dark_stops` sign clash (P8) is
`core/rename-fit-range-misnomers`', which renames the same `dark_end` report block; the
American spellings (P13) stay, since identifiers keep their spelling and `gray-world` is
already a retired value.

## Design

Decided: no version reset is planned, so renamed recipe keys migrate now — the old
`null` default is stripped, any other value refused naming the new key, with no alias.
Report fields are renamed without a migration. New public names spell "colour".

## How to Verify

- Every old recipe key and flag is refused with a migration message naming the new one
  (`recipe::check_body`, `cli::reject_removed_flags`), tested through the binary.
- `docs/terminology.md`'s "Known public misnomers" loses the rows, and
  `docs/using-nc.md` is re-verified against the binary.
- CI gates green; `--save-recipe` output carries only the new names.
