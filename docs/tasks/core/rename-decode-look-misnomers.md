---
title: Rename the decode and look misnomers (--density-gamma, reconstruction.anchor, look stage "contrast")
status: done
deps: [core/value-domain-terminology]
summary: public names that say the wrong quantity in the decode and the look
done: 2026-10-15
---
# Rename the decode and look misnomers

## Goal

Give three public names the quantity they actually carry (`docs/terminology.md`,
"Known public misnomers"):

- `--density-gamma` sets the linearization, not a gamma (the recipe key already says
  `linearization`).
- `reconstruction.anchor` holds `d` (mid-grey above base) while `chain.decode.anchor`
  holds `A`: one leaf name, two quantities.
- The look's stage string `"contrast"` fires on tone slope ≠ 1, so a default run
  reports "contrast" with `look.contrast` = 1.

## Design

Decided (2026-10-15): `--linearization`; `reconstruction.anchor_rule`, the old key
refused at every value, its old default included (a user decision against the
strip-the-default rule); the look's stage strings `tone-slope` and `saturation-slope`,
report-only. No version reset is planned; no `pipeline_version` bump.

## How to Verify

- Every old spelling is refused with a migration message naming the new one
  (`recipe::check_body`, `cli::reject_removed_flags`), tested through the binary.
- `docs/terminology.md`'s "Known public misnomers" loses the rows, and
  `docs/using-nc.md` is re-verified against the binary.
- CI gates green; `--save-recipe` output carries only the new names.
