# Separate taste from quality in the automatic adjustments

## Goal

Classify every adjustment Hanten applies on its own as **quality** (it corrects the scan
toward what the film recorded: the roll's white balance, its exposure, the white rule's
placement) or **taste** (a preference: `frame-level-trim`'s small lift, `thin-frame-lift`'s
thin lift). Every taste adjustment gets one documented off switch, and the classification
records what a future GUI needs to preview it and turn it off.

Filed by the user on 2026-10-01: `thin-frame-lift`'s round 2 could not be called, because
the lift is brighter and whether that is better is opinion, not fact.

## Design

Known when filed:

- Today both lifts are turned off together, at render by `--frame-lift off` /
  `roll.frame_lift` and at measurement by `measure-roll --no-frame-lift`;
  `--no-thin-lift` keeps the small lift and drops only the thin one. No render-time switch
  separates the two.
- Both lifts are on by default. The flat-frame guard (`roll_white::FLAT_SPREAD_STOPS`)
  keeps both off a single-surface frame.

Decided 2026-10-02 (user):

- **The line**: a correction restores what the roll recorded, on every frame alike (white
  balance, exposure, the white); a guard bounds a measurement or keeps a preference off
  (the white's cap, floor and clamp; the flat frame); a preference reacts to one frame
  (both lifts). Design-spec §6, "Corrections and preferences".
- **One switch each**, not one "taste" switch: `--small-lift` / `roll.small_lift` and
  `measure-roll --no-small-lift` (small), and `--thin-lift` / `roll.thin_lift` and
  `--no-thin-lift` (thin). The recipe stores a thin frame's lift as its own pair
  (`roll.thin_slope`, `roll.thin_exposure`) beside the small lift, so thin off renders the
  small one.
- **The old switch is retired, not reused** (user): `--frame-lift`, `roll.frame_lift` and
  `--no-frame-lift` turned both lifts off, so keeping the name for the small lift alone
  would change what an old command renders. Each is refused with a migration to both
  switches; a `null` key is dropped.
- **`default` applies the preferences that won review**, each marked and switchable; the
  spec's "no taste" was amended, not the defaults.
- **Marking**: `chain.roll.taste_applied`, and `"kind": "taste"` on `measure-roll`'s two
  lift sections.
- **Carried over** to `thin-lift-confirmation`: the independent roll exists now (09-29).

## How to Verify

- Every automatic adjustment is classified in one place (the design spec), with a reason.
- Every taste adjustment can be turned off at render without re-measuring, by flag and by
  recipe key, and the guide documents each switch.

## Dependencies

- [A bounded lift for a thin frame](thin-frame-lift.md) — the thin lift is a taste
  adjustment, and its switch is part of what is classified
- [A per-frame level trim](frame-level-trim.md) — the small lift, the other taste
  adjustment
