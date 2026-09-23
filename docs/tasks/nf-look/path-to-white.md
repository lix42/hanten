# Highlight desaturation

## Goal

Chroma goes to zero as a pixel approaches white, as a deliberate, parameterised
part of the look — anchored at diffuse white rather than at a branch's display
white.

## Design

- **The form is settled: a chroma pull, keyed on brightness *and* saturation.**
  [The spike](desaturation-spike.md) (report: [`docs/spike/highlight-desaturation.md`](../../spike/highlight-desaturation.md)) compared it against a per-channel curve — what film,
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
  1.0, with a ceiling that follows the rendered luminance). Separating their shares is
  [its own task](../nf-display-stages/gamut-map-share.md); this task then makes the
  deliberate half deliberate.
- **Build it under a hand-set contrast, and treat the band's numbers as provisional
  (decided 2026-09-22).** Under the base-referenced anchor the operator is inert — all
  three measured rolls land 0.55–1.28 stops below white
  ([`docs/spike/white-placement.md`](../../spike/white-placement.md)), so nothing enters
  the range it acts in. The lift comes from letting the roll's own content drive
  **contrast** rather than moving a level: candidates **C** (pin mid *and* solve
  `gamma = MID_GREY_OUTPUT_DECADES / (W − d)`, reaching white by construction) and **D**
  (C with a gamma ceiling, sliding toward B when it binds, so the noise budget is an
  explicit parameter rather than an accident — C asks Gold200 for 4.15 where the outside
  converters measured 3.07–3.18, and `corr(slope, noise) = 0.89`).

  That rule is [`nf-calibration/anchor-comparison`](../nf-calibration/anchor-comparison.md)'s
  to choose, and it runs **after** this task
  ([`anchor-rule`](../nf-reconstruction/anchor-rule.md) froze only `d`). So this task is built and
  verified against a **hand-set per-roll contrast** — flags only, no new code:
  `--density-curve exponential --anchor-mid-offset <d> --density-gamma <g>`, with `g`
  computed per roll from that roll's measured base and its red p97 (the arithmetic and
  the raw data are in the anchor spike's working directory). That reaches candidate C
  **exactly**, not approximately: `mid-at-base-offset` resolves
  `A = d + MID_GREY_OUTPUT_DECADES / gamma`, and C's `gamma = MID_GREY_OUTPUT_DECADES /
  (W − d)` substitutes into it to give `A = W`. The mid-anchored and content-anchored
  spellings are the same placement — `white-placement.md`'s framing correction arriving a
  second time. Solved the other way (`d = W − M/gamma`, giving 0.428 / 0.538 / 0.488 on
  the three rolls — the spike's own table) the same two knobs reach **B** as well, so this
  task can be tuned under more than one candidate white if that turns out to matter.

  **A caution about tuning only there.** The exponential is the right control because `A`
  is the density mapping to white *on the straight line*, so a knee'd curve's shoulder
  compresses above it and `A = W` would not put rendered white at 1.0. But that means the
  band is fitted on a render with **no per-channel shoulder** — one of the only two
  surviving candidates for the knee'd render's clean whites (design-update Appendix F).
  Check the fitted band under a knee'd render before shipping it, or record why not.
  (The sigmoid carries the same `AnchorPlacement`, and `--sigmoid-contrast` is the
  `--density-gamma` analogue, so the equivalent line exists.) The **shape** of the
  operator is what this task ships; its parameters are re-fitted once the anchor rule is
  chosen. Do not bank the band values against the spike's per-frame p97 white, which is
  a *level* move: a contrast move steepens everything below white too, so a different
  population of pixels lands in the operator's range.
- **The threshold is scene-referred, and the spike's was not.** The throwaway operator
  started at *rendered* linear luminance 0.5 (≈ L\* 76), i.e. after fit range. The design
  requires the trigger at **diffuse white, pre-branch**. Once the decode pins mid — and
  on a straight line a mid anchor and a white anchor are one rule, which is
  `white-placement.md`'s framing correction — diffuse white is a known value at the
  decode's output, so express the threshold against that. The operator's anchor *is* the
  decode's anchor.
- **Near-cycle worth knowing about.**
  [`nf-calibration/anchor-comparison`](../nf-calibration/anchor-comparison.md) depends on
  this task (only a render with a real highlight operator can rank the white placements),
  while this task needs an anchor that reaches white. The task graph stays acyclic and no
  edge records that second direction — the coupling is the hand-set contrast above. (The
  executability answer is otherwise unremarkable: this task now needs three `[x]` deps,
  not just the look stage — `desaturation-band-fit` and `gamut-map-share` were added the
  same day and both can run immediately.)

## Open questions

- **The functional form.** The spike used a linear band over linear-RGB `(max−min)/max`,
  full pull below `s0` and off above `s1`. Its **numbers** are
  [their own task](desaturation-band-fit.md) — 0.30 → 0.45 was fitted to one saturated
  patch on one roll, so the shape carries and the values do not.
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
- If this is the **first look control to land**: `LookParams` gains a "non-empty"
  predicate, and `applied()` and any destination that runs no look (`film-master`,
  once the new flow has one) read it — one rule, never one per knob
  (`nf-look/stage`). A look key on a no-look destination refuses, naming the look
  — if the new flow has one by then; otherwise `nf-destinations/preset-set`
  verifies it.

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
  — **done 2026-09-21**, and it moved this task's premise: under the base-referenced
  anchor the operator is **inert**, so this task is built against a hand-set contrast
  (see Design). Whether the anchor also produces the whites is
  `nf-calibration/anchor-comparison`'s, and it depends on this task
- [Fit the desaturation band on more than one patch](desaturation-band-fit.md)
  — the values this task ships
- [Separate the gamut map's share](../nf-display-stages/gamut-map-share.md)
  — how much of the convergence is already happening before this operator exists
