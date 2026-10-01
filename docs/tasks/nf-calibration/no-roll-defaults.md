# The default rendering without a roll measurement

## Goal

Choose the values a render uses when no roll measurement is given, so that a frame
converted without `hanten measure-roll` lands on the best common ground across rolls.
These are the `default` rendering's fallbacks (`nf-destinations/direct-preset`); a roll
measurement replaces them.

## Design

What is known:

- **The first value is the default whole contrast.** When filed it was `2.0`, the
  bundled decode's: the linearization `1.8` times a fallback slope `2.0 / 1.8 ≈ 1.11`,
  kept for continuity, never tuned.
- **The roll-measured whole contrast runs 2.23–2.97** on the nine reviewed rolls
  (`anchor-comparison`, `roll-white-rule`), all above the old fallback. The user has a
  strong prior to raise it, to about `2.5` (slope ≈ 1.39 at the 1.8
  linearization; 2026-09-27).
- **Moving it moves only renders without a roll white** (`nf-look/contrast-definition`):
  `look.contrast` is a multiplier on this base, so a stated contrast moves with it too.
- **`direct` does not follow this default by itself.** Its contrast is a separate
  pinned value (`direct-preset`), so it moves only when decided explicitly.

Decided (2026-09-30, progress log):

- **The fallback is a white**, `roll_white::FALLBACK_WHITE_STOPS` = **+1.75** (whole
  contrast 2.54), chosen by review over 2.0 and the floor; the slope is `slope_for` of it.
- **`direct`'s pinned slope moved to the same value**, logged as a move of the held
  rendering.
- **White balance stays neutral and exposure 0** without a measurement — the other two
  values a `roll` section supplies (`roll.exposure` arrived after this task was filed).
  Highlight desaturation and display black are the `default` rendering's defaults
  whether or not a roll is measured, so tuning them is their own tasks'.

## How to Verify

- A recorded verdict on the whole-contrast fallback, from the measured per-roll values
  and a review of frames rendered without a roll measurement.
- Any pixel move carries its `pipeline_version` bump and fingerprint row if it changes a
  default render.

## Dependencies

- [`measure-roll` places the roll's white](roll-white-rule.md) — the per-roll contrasts
  the fallback is chosen against
