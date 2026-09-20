# Retire the regional balance

## Goal

Remove `shadow_balance` / `highlight_balance` and the `balance_range` machinery
that anchors their tone ramps, now that the look stage carries a pivoted
per-channel grade.

## Design

- **They are the same family, spelled worse.** Per-channel density offsets ramped
  by tone region are per-channel adjustment by tone region; the grade
  (`out_c = mid · (in_c / mid)^k_c`) is one control where these are three, and it
  acts on working-space channels rather than on film density before the 3×3,
  where changing one channel moves all three outputs.
- **They can be non-monotone.** Nothing bounds their magnitude, so a large enough
  difference between the two ends maps two scene densities onto one — an
  inversion no counter reports. That is a reason to retire rather than to port.
- **The substantive difference is the measurement.** `BalanceRange::Auto`
  measures `[lo, hi]` per frame from the corrected densities, and roll
  consistency then depends on a measure-once-replay ritual; the grade's pivot is
  a fixed 0.18 and needs no measurement at all. Say that in the migration note
  rather than describing the grade as a rename — it is one fewer per-frame
  measurement in the decode, which is the direction the design pushes.
- Removal follows the `algorithm` precedent: a migration error, no aliases, and
  the report loses its measured-range field rather than reporting a constant.

## How to Verify

- The flags and recipe keys are gone, and a recipe using them names the grade in
  its error.
- No report field or `--dump-params` output still carries a balance range.
- A frame that previously used them renders through the grade with the difference
  documented — not asserted equal.

## Dependencies

- [A per-channel grade with a pivot](../nf-look/per-channel-grade.md)
