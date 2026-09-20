# Fit range as one stage

## Goal

Make fitting the scene's range into the display's range a single named stage that
both display branches call, with reinhard as the baseline setting. Today that job
is spread across two hand-written per-pixel bodies and has no name of its own.

## Design

What is known:

- **The duplication is real and it is three-in-one.** `pipeline::sdr::render_pixel`
  and its HDR counterpart each fuse the tone selection, the luminance rescale and
  the gamut map into one loop, once per branch. `display_tone` already resolves the
  tone *value* both read, so what is missing is the stage, not the parameter.
- **Written fresh.** Per CLAUDE.md's migration rule, do not extract a stage out of
  those bodies because that is where the arithmetic happens to live.
- **Reinhard is the baseline, not the only option.** `shoulder` and `none` exist for
  reconstructions already bounded at white and go away with `highlight_compress`
  (design-update Part 2 decisions); removing them is `nf-retire`'s work, but this
  stage should not be designed around them.
- **The display's peak is the stage's parameter**, which is what makes SDR and HDR
  the same function with different arguments rather than two renderers.

Open:

- **What the stage is called in the recipe.** The `print.*` prefix is a legacy-path
  leftover and is being renamed; this stage is the first that needs a new home.
- **`print.linear_range` is an affine levels remap, not fit range.** It needs a name
  and a stage, probably scene correction — settle with that epic rather than
  absorbing it here.
- **The black point splits in two** (flare in scene correction, display black here).
  Whether the display-black half is a subtraction at all or a property of the
  operator is [the parametric operator](parametric-operator.md)'s question.
- Whether headroom keeps being spelled in stops, and whether the stage reports the
  resolved operator by name rather than by prose claim (CLAUDE.md's "fifth spot").

## How to Verify

- One implementation, called by both branches; nothing renders tone anywhere else.
- Goldens written for the new stage rather than inherited from the legacy vectors.
- A report field says which operator and which peak were resolved, and a run with a
  non-default operator does not leave a stale prose claim behind it.

## Dependencies

- [The look stage](../nf-look/stage.md) — fit range is the first stage after the
  look, and the look's output is its input contract
