# What `look.contrast` means

## Goal

Restate `look.contrast` so its number is unambiguous, and so an explicit contrast can
build on top of the roll's measurement the way `--white-balance` does.

## Design

What is known (`docs/design-update.md`, Part 2, "Two renderings", 2026-09-27):

- **Three contrasts are in play**: the whole contrast (2.0 bundled; 2.23–2.97
  roll-measured), the rendering contrast `look.contrast` (`2.0 / 1.8 ≈ 1.11` by default;
  what `measure-roll`'s `contrast_for` computes), and the decode's linearization (1.8).
  The whole contrast is `look.contrast × reconstruction.linearization`.
- **Today's meaning is kept until this task**, because `measure-roll`'s output and every
  recipe written since `nf-calibration/roll-white-rule` use it. Under it an explicit
  `--contrast` replaces the rendering's base instead of building on it.
- The two candidates: `look.contrast` as the **whole** contrast (2.0 reads as 2.0), or as
  a **multiplier** on the rendering's base with identity 1.0 (so it composes).

Decided (user, 2026-09-28):

- **A multiplier.** `look.contrast` / `--contrast` keeps its name and multiplies the
  base slope; default 1 keeps the base. So a stated contrast builds on the roll's
  exactly as `--white-balance` builds on the roll's gains, and a taste carries across
  rolls as one number.
- **Only the knob is called contrast.** The absolute values are **slopes**: `slope`
  (scene to output, 1 reproducing the scene), reported with `base_slope` and
  `base_from` (`roll`, `fallback`, `direct`); `look::DEFAULT_SLOPE` is the no-roll
  fallback. The whole slope (× linearization) is internal only.
- **Not the white in stops.** `roll.white_stops` stays the stored measurement, but it
  is not the user's control: a higher white is a flatter picture, the opposite of how
  "more stops" reads.
- **`recipe_version` 3.** Version 2 is still read; a version 2 `look.contrast` number
  is refused with the multiplier that keeps it (against the recipe's own rendering and
  roll white), and its `null` reads as 1. The user plans to reset every version before
  the first release.
- The fallback warning's slope half is silenced as white balance's is: a typed
  `--contrast`, or a recipe value off 1.

## How to Verify

- An old recipe carrying `look.contrast` either renders as before or is refused with a
  migration message; it never renders differently in silence.
- Under the chosen meaning, an explicit contrast under `--rendering default` builds on
  the roll's contrast, and the report states both.

## Dependencies

- [Two renderings: `direct` and `default`](../nf-destinations/direct-preset.md) — the
  bases a contrast builds on
