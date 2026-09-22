# Highlight desaturation

## Goal

Chroma goes to zero as a pixel approaches white, as a deliberate, parameterised
part of the look — anchored at diffuse white rather than at a branch's display
white.

## Design

- **The form is settled: a chroma pull, keyed on brightness *and* saturation.**
  [The spike](desaturation-spike.md) compared it against a per-channel curve — what film,
  paper and all three outside converters do — and found them perceptually equivalent at
  matched cleanup and matched brightness (ΔE ≈ 0.7). The pull wins on **separability**:
  a per-channel curve moves luminance as well as chroma, and the fit range downstream
  eats whatever exposure compensates it, so the two jobs cannot be tuned apart.
- **Strength must key on distance from the neutral axis, not on brightness alone.** With
  brightness alone the operator neutralises a bright *coloured* surface as hard as a
  bright *white* one — measured on a sand beach at C\* 27.5 → 1.1. A saturation band
  multiplying the brightness term left the whites identical (C\* 6.1 either way) and kept
  the sand at 22.3, confirmed by eye. **This is the spike's main result**, and no
  aggregate found it: one marked patch did.
- **It is a highlight operator and cannot be more.** Its reach is its threshold —
  surfaces at L\* 64–68 were untouched at every setting tried. Cast there belongs to
  [the per-channel grade](per-channel-grade.md) or to the decode's `scale`, and judging
  "are the whites clean" on a frame whose white sits at L\* 64 measures the decode rather
  than this stage.
- **Anchored at diffuse white, and that is forced rather than preferred.** The branches
  compress against different ceilings — 1.0 for SDR against `LINEAR_HEADROOM` ≈ 4.93
  for HDR — so the same operator placed *per branch* converges hard on SDR and barely
  at all on HDR, where a typical frame's ~1 stop above diffuse white never approaches
  the ceiling. The same frame would read neutral on one display and cast on the other,
  and a gain map would not complain, since it requires agreement only *below* diffuse
  white. Diffuse white is scene-referred and common to both, so run once before the
  branch both renditions inherit the same convergence.
- **It does not replace fit range.** The two are orthogonal and separated for opposite
  reasons: **chroma** convergence must be pre-branch so the branches agree on white,
  while **luminance** compression must stay per-branch so HDR can carry more than SDR.
  Collapsing them into one per-channel fit-range operator gives up the second to get the
  first. What should shrink instead is the gamut map's incidental share.
- **Parameterised, with "off" available.** How early it starts and how hard it
  pulls are look choices; off matters because desaturation *hides* residual cast,
  which is right for a print and wrong for a diagnostic render or for judging a
  decode (Part 3 makes the same point about comparing decodes).
- **It must not double up with the gamut map** — which is also one of the two live
  candidates for the knee'd render's whites (`sdr.rs:249-266`, radial near luminance
  1.0, with a ceiling that follows the rendered luminance). The spike separates their
  shares; this task then makes the deliberate half deliberate.

## Open questions

- **The functional form and its parameters.** The spike used a linear band over
  linear-RGB `(max−min)/max`, full pull below `s0` and off above `s1`, placed at
  0.30 → 0.45 — but that band was fitted to **one** saturated patch on one roll, so the
  shape carries and the numbers do not. A perceptual saturation measure may separate the
  cases better than a linear-RGB one.
- **Whether the pull should preserve hue exactly.** The spike's lerp toward `(Y, Y, Y)`
  holds luminance to 3e-3 but still rotates hue 4.3°, because a straight line to the
  achromatic point in linear ACEScg is not a constant-hue path in CIELAB. Nothing
  suggests that rotation was visible; a perceptual construction would cost more.
- Whether anything is applied above diffuse white per branch, or the operator
  stops there entirely.
- **Whether the "direct" preset should get it.** Part 2 wants that preset minimal and
  continuing in Lightroom, but an operator that hides residual cast is the wrong default
  where the cast is what a user is about to correct. A switch makes this a choice; had
  it lived in fit range it would not have been one.

## How to Verify

- Off is a bit-exact identity.
- A saturated near-white patch desaturates monotonically as the parameter rises.
- On a real frame, top-end chroma on **marked neutral patches** falls as the parameter
  rises (the review app measures them natively since #128), **while a marked saturated
  patch does not** — that pair is the check, and either alone passes a broken operator.
  Appendix E's decile columns are not the check: they carry one frame's own scene colour,
  so only their trend is comparable, and the causal reading attached to them was retired.
- The SDR and HDR renditions still agree below diffuse white.

## Dependencies

- [The look stage](stage.md)
- [A `density.scale` ladder, before the calibration frames exist](../nf-calibration/scale-ladder.md)
  — **done 2026-09-20**, and it did not settle this task's status: the shipped scale
  proved to be at the optimum of what one global value can do, so no scale was going
  to reach the knee'd render's whites by itself. The evidence moved to the anchor
  instead (see Design), which is why this task's premise needs re-reading before it is
  picked up.
- [Spike: what form should highlight desaturation take?](desaturation-spike.md)
  — **done 2026-09-21.** Settled the form (a chroma pull, keyed on brightness and
  saturation) and left the gamut map's share unseparated; see Design
- [Spike: does a diffuse-white anchor earn its place?](../nf-reconstruction/anchor-spike.md)
  — if the anchor produces the whites, this task is an optional look rather than a
  remedy for cast
