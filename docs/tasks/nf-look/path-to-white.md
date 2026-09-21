# Highlight desaturation

## Goal

Chroma goes to zero as a pixel approaches white, as a deliberate, parameterised
part of the look — anchored at diffuse white rather than at a branch's display
white.

## Design

- **Why it *might* have to be explicit — the premise is narrower than it was filed on.**
  The 2026-09-17 round (design-update Appendix E) measured the knee'd sigmoid's channels
  converging as lightness rises, B/R 1.65 → 1.27 against 1.76 → 1.48 for the
  shoulder-less render, and read the per-channel shoulder as the cause. It listed four
  differences between the two presets; **three of them die by algebra** (Appendix F):
  `print_exposure` is a scalar gain after the curve and the anchor factors out as a
  common gain, while every nc display tone is luminance-preserving. Two candidates
  remain — the **per-channel shoulder** and the **gamut map**'s radial convergence near
  luminance 1.0 — and both are per-channel-ish, so the mechanism is real either way.
  What is *not* yet known is which of the two the eye was responding to.
- **The anchor positions; it does not converge.** A common gain cannot move a channel
  ratio, so the three converters' neutral whites are not the anchor's doing either —
  their shoulders run **per channel against a common ceiling**, each channel
  asymptoting to the same value so one arriving higher is compressed more (SF measured
  1.13× apart at p97, 1.05× at p99.5). The anchor's contribution is deciding how much
  content lands where that acts. So this task and
  [`nf-reconstruction/anchor-rule`](../nf-reconstruction/anchor-rule.md) are two halves
  of one mechanism, which is why
  [the spike](../nf-reconstruction/anchor-spike.md) tests them together.
- **Nothing in nc can do this today, so it is new code.** `sdr.rs:248` and `hdr.rs:550`
  both curve one luminance and multiply all three channels by the ratio, so `shoulder`,
  `reinhard` and `none` alike leave every ratio invariant — none of them can be
  repurposed. The only per-channel nonlinearity nc has above diffuse white is the
  sigmoid's shoulder, which the new design removes from reconstruction.
- **A cost to weigh that was not previously recorded.** Per-channel highlight
  compression pulls *saturated* highlights toward white, and a sunset is saturated
  highlights. Whatever makes whites clean is the same mechanism that flattens a
  sunset, which is a second, independent argument for the parameterisation below.
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

- What "approaches white" is measured on — luminance, max channel, or a
  saturation measure — and whether the pull preserves hue.
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
  rises (the review app measures them natively since #128). Appendix E's decile columns
  are not the check: they carry one frame's own scene colour, so only their trend is
  comparable, and the causal reading attached to them was retired.
- The SDR and HDR renditions still agree below diffuse white.

## Dependencies

- [The look stage](stage.md)
- [A `density.scale` ladder, before the calibration frames exist](../nf-calibration/scale-ladder.md)
  — **done 2026-09-20**, and it did not settle this task's status: the shipped scale
  proved to be at the optimum of what one global value can do, so no scale was going
  to reach the knee'd render's whites by itself. The evidence moved to the anchor
  instead (see Design), which is why this task's premise needs re-reading before it is
  picked up.
- [Spike: does a diffuse-white anchor earn its place?](../nf-reconstruction/anchor-spike.md)
  — if the anchor produces the whites, this task is an optional look rather than a
  remedy for cast
