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

Open:

- Which of the two, or something else.
- How recipes written under the current meaning are handled: a key that changes meaning
  in place renders an old recipe differently with nothing to catch it (CLAUDE.md,
  retiring a recipe key), so this is a rename, a refusal with a migration message, or a
  `recipe_version` bump.

## How to Verify

- An old recipe carrying `look.contrast` either renders as before or is refused with a
  migration message; it never renders differently in silence.
- Under the chosen meaning, an explicit contrast under `--rendering default` builds on
  the roll's contrast, and the report states both.

## Dependencies

- [Two renderings: `direct` and `default`](../nf-destinations/direct-preset.md) — the
  bases a contrast builds on
