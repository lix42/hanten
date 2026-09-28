# The default rendering without a roll measurement

## Goal

Choose the values a render uses when no roll measurement is given, so that a frame
converted without `hanten measure-roll` lands on the best common ground across rolls.
These are the `default` rendering's fallbacks (`nf-destinations/direct-preset`); a roll
measurement replaces them.

## Design

What is known:

- **The first value is the default whole contrast.** Today it is `2.0`, which is
  `algo::fixed::BUNDLED_CONTRAST`: the decode's linearization `1.8` times the look's
  default `look.contrast` of `2.0 / 1.8 ≈ 1.11`. It was kept so that the new flow renders
  a neutral where the bundled decode did — a continuity choice, never a tuned one.
- **The roll-measured whole contrast runs 2.23–2.97** on the nine reviewed rolls
  (`anchor-comparison`, `roll-white-rule`), all above the fallback. The user has a
  strong prior to raise it, to about `2.5` (`look.contrast ≈ 1.39` at the 1.8
  linearization; 2026-09-27).
- **Whole contrast, not `look.contrast`, is the number to choose.** The look's default is
  derived as `whole / LINEARIZATION`, so the whole contrast holds when
  `scale-gamma-loop` moves the linearization.
- **`direct` does not follow this default by itself.** Its contrast is a separate
  pinned value (`2.0 / 1.8`, `direct-preset`), so moving the fallback does not move the
  rendering the calibration loop holds fixed.

Open:

- Whether `direct`'s pinned contrast moves with the fallback: decided here, explicitly,
  and logged as a move of the held rendering if it does.
- The white balance's fallback (neutral today) is the one other value in scope: only
  the white balance and the contrast have a roll measurement to fall back from.
  Highlight desaturation and display black are the `default` rendering's defaults
  whether or not a roll is measured, so tuning them is their own tasks'
  (`nf-look/desaturation-band-refit`, `nf-display-stages/parametric-shoulder`), never
  a value that changes with the presence of a `roll` section.
- Whether to choose the contrast before `scale-gamma-loop` settles the decode, or after.
- How to judge a fallback: a single value against every roll's measured contrast, or a
  review of frames rendered without a measurement.

## How to Verify

- A recorded verdict on the whole-contrast fallback, from the measured per-roll values
  and a review of frames rendered without a roll measurement.
- Any pixel move carries its `pipeline_version` bump and fingerprint row if it changes a
  default render.

## Dependencies

- [`measure-roll` places the roll's white](roll-white-rule.md) — the per-roll contrasts
  the fallback is chosen against
