# The scene-referred half of the black point

**Closed — not needed (2026-09-26).** There is no scene-referred additive term left
for scene correction to remove, and the one job the legacy black point did is display
black's (`nf-display-stages/parametric-operator`).

## Goal

Give flare/fog removal a place of its own in scene correction, so that reaching
black at the display end stops being done by subtracting from scene-referred
values.

## Outcome

The premise was that `print.black_point` did two jobs: placing black on the display,
and removing an additive veil the lens and scanner contribute. None of the three
candidate terms belongs in this stage:

- **Base fog is already removed.** It is part of the unexposed film, so the measured
  film base (from the rebate) includes it, and the decode references density to that
  base.
- **Camera lens glare is part of the photograph**, light that reached the film, not
  a correction toward the scene. If it is ever wanted, it is an opt-in look control,
  never a scene-correction default.
- **Scanner veil acts in the transmission domain**, where it matters most in the
  negative's dense areas (the scene's highlights). The clear base barely feels it.
  If it is ever addressed, it is a scanner-calibration and highlight question, not
  a black one.

The evidence agrees: in `nf-calibration/anchor-comparison`, every roll's darkest
pixels sat at the film base itself (red p0.5 at −3.6 to −3.8 scene stops, base at
−3.7), so there is no pedestal in the shadows to remove. The legacy `--black-point`
was therefore one job, placing black, and `--display-black`
(`fit_range.display_black`) now does it.

Consequences:

- Scene correction stays white balance and exposure.
- Nothing upstream of the look produces negative channels, so the per-channel grade's
  whole-pixel guard (`nf-look/per-channel-grade`) stays latent. Whatever stage first
  produces negatives must revisit it.
- Display black's reference (the film base graded with the frame) has no scene
  subtraction to account for.

## Dependencies

- [Scene correction as a named stage](stage.md)
