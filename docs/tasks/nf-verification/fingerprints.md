# Rebase the drift gate on the new chain

## Goal

Rework `version::PIPELINE_FINGERPRINTS` so its rows describe the new chain rather
than the legacy print path, without losing the property the gate exists for: a
default cannot move inside the fingerprinted stages without the version moving
with it.

## Design

- **The print half is already gone.** `nf-core/default-flip` made v8's `render`
  hash the fixed decode; `base` and `recipe` never involved the print path.
- **Where the `render` hash stops — decided (2026-09-28): at scene correction's
  input** (the decode plus the NC film RGB v1 → ACEScg mapping). Every rendering
  stage after it makes libm calls at its defaults (the look's `powf`, highlight
  desaturation, display black), so the stage-goldens note that everything
  downstream is IEEE-only no longer holds. The rendering stages' values stay
  `recipe`'s and their arithmetic `chain_golden`'s.
- **Supersedes `algo/characteristic-fingerprint-vector`.** A fingerprint has **no
  window**, so a sample whose decode window is wider than the final `powf`'s one
  ULP is designed out of the vector rather than bounded. Only samples within about a
  third of a stop of the base reach that minimum; midtones, dense and floored samples reach
  4–37 ULP.
- **Recorded in place, not bumped (user decision).** No default pixel moves, so
  v8's `render` is refreshed with a comment rather than a v9 carrying an identical
  render. Rows v1–v7 are untouched.

## How to Verify

- v8's row is computed on both CI targets and agrees; a deliberate default change
  inside the covered stages fails the gate with the copy-pasteable message.
- Historical rows and their `behavior` strings are untouched.

## Dependencies

- [A minimal end-to-end render](../nf-core/minimal-end-to-end.md)
