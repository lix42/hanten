# A per-channel grade with a mid-grey pivot

## Goal

The look stage carries a per-channel adjustment pivoted at mid-grey — the
photographer-facing counterpart of the decode's `scale`, acting on working-space
channels after the 3×3.

## Design

- **It addresses the symptom, not the error, and that is the point.**
  Reconstruction's `scale` acts on film layers before the 3×3, so moving one
  channel there moves all three output channels: accurate, and unpredictable to
  tune by hand. A grade after the matrix moves the channel a photographer is
  looking at (design-update Part 2, "A per-channel grade with a pivot").
- **The pivot is what makes it more than white balance.** Without one, a
  per-channel power moves neutral everywhere; pivoted at mid-grey, a neutral mid
  stays neutral and the cast grows away from mid in both directions — which is
  exactly what white balance cannot do.
- **It subsumes `shadow_balance` / `highlight_balance`.** Per-channel adjustment
  by tone region is the same family, and one control beats three. Those two are
  also the non-monotone term in today's chain (Part 1), so retiring them into this
  is a monotonicity win as well as a simplification.
- **It does not replace the calibration.** Without a measured `scale` every roll
  needs hand-grading; this is a grade on top of a calibrated decode.
- **Settled by `nf-look/contrast` (2026-09-24):**
  - **It runs after contrast and before highlight desaturation** (user). Against
    contrast the order is free — two pivoted powers compose by multiplying exponents, so
    channel `c` ends at `contrast · g_c`. Against desaturation it is not: running first
    suits the grade's main job, fixing crossover, at the cost that a deliberate creative
    highlight cast is partly pulled back toward neutral.
  - **A grade's cast grows with contrast**, and should: the grade's exponents compose
    with the contrast's, so the channel difference they make is multiplied by it too. A
    crossover the decode leaves is an exponent mismatch between layers, which the
    look's contrast multiplies too, so a grade that scales with it tracks the error;
    dividing by contrast would under-correct a high-contrast roll. Under a per-roll contrast (`anchor-comparison`'s C/D,
    1.43–3.67×) the same grade values therefore read stronger on a contrastier roll.
- **Needs a guard at and below zero.** Contrast passes non-positive and non-finite
  samples through untouched (`look::apply_contrast`); matching that keeps the look's two
  powers alike at the gamut edge. A wide-gamut linear working space contains
  negative components, and a fractional power of a negative is NaN. Decide the
  behaviour deliberately — clamp, reflect through the pivot, or pass through — and
  report which; a NaN reaching the encoder is only counted, not explained.

## Open questions

- The grade's form inside its `look` key: a pivoted per-channel power, or a
  lift/gamma/gain triple per channel? The container is settled (`nf-look/stage`,
  2026-09-23): one key per control under `look`, not one CDL object — CDL's slope
  is white balance and its offset the flare subtraction, both scene correction's.
- **How it stays off neutral contrast.** Equal exponents on all three channels, pivoted
  at mid, *are* contrast — the same duplication that ruled out CDL. Contrast owns
  neutral contrast (`nf-look/contrast`), so the grade must not be able to move it: a
  neutral's luminance slope must stay at the contrast. Constraints that weight the
  channels equally (exponents multiplying to 1, or held relative to green) do not meet
  that, since luminance weights the channels unequally. Which form does is this task's
  (user, 2026-09-24).
- **Should desaturation's band account for the grade?** The band divides
  `log10(max/min)` by linearization × contrast only (`look`'s `Pull`); running between
  the two, the grade changes channel ratios before the band classifies a pixel.
- The pivot: contrast fixes it at `look::MID_GREY`, the value the decode's anchor pins
  mid at; the grade should share that pivot.

## How to Verify

- Unit exponents are a bit-exact identity.
- A neutral mid-grey pixel stays neutral at any setting; departure from neutral
  grows away from mid in both directions on a synthetic ramp.
- Zero and negative components stay finite, in the documented way.
- A synthetic cast measurably shrinks, read back with `nctool metrics`.
- The first look control has landed (`path-to-white`, highlight desaturation on by
  default): `LookSection` carries two predicates, `is_empty` (moves no pixel — what
  `applied()` reads) and `asks_for_a_look` (neither default nor empty — what a no-look
  destination such as `film-master` reads to refuse: the default is spared because
  every default recipe carries it, an empty look because it is an identity). This
  control extends both.

## Dependencies

- [The look stage](stage.md)
