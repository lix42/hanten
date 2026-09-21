# Highlight desaturation

## Goal

Chroma goes to zero as a pixel approaches white, as a deliberate, parameterised
part of the look — anchored at diffuse white rather than at a branch's display
white.

## Design

- **Why it *might* have to be explicit — the premise is weaker than it looks.** The
  2026-09-17 round (design-update Appendix E) measured the knee'd sigmoid's channels
  converging as lightness rises, B/R 1.65 → 1.27 against 1.76 → 1.48 for the
  shoulder-less render. It did **not** establish the shoulder as the cause: the two
  presets differ in four ways at once (shoulder, anchor 0.28 vs 0.5 mid-fraction,
  `print_exposure`, `display_tone`). What is settled is only the negative half — a
  luminance-preserving operator scales all three channels by one factor, so it cannot
  converge them at all and a cast survives to display white.
- **And the 2026-09-20 measurements point at the anchor instead.** All three outside
  converters produce neutral whites without any highlight-desaturation operator
  (`docs/reports/three-way-gold200.md`): their channels converge because the anchor
  sits high in density with a shoulder and a ceiling above it. SilverFast reaches a
  near-neutral white with a *fixed* per-channel calibration and no colour step at all.
  If that mechanism is what the eye was responding to, it belongs to
  [`nf-reconstruction/anchor-rule`](../nf-reconstruction/anchor-rule.md) and this task
  is an **optional look**, not a remedy for cast — which is the reading the design's
  own "off must stay available" clause already implies, since an operator that hides
  residual cast cannot also be the fix for it.
- **A cost to weigh that was not previously recorded.** Per-channel highlight
  compression pulls *saturated* highlights toward white, and a sunset is saturated
  highlights. Whatever makes whites clean is the same mechanism that flattens a
  sunset, which is a second, independent argument for the parameterisation below.
- **Anchored at diffuse white.** Display white differs between SDR and HDR, so a
  single pre-branch operator cannot be defined against it. Diffuse white is
  scene-referred and common to both, and a gain map only requires the renditions
  to agree *below* it — the same crossover the HDR highlight lift already uses. So
  the trigger point is shared even where the pull above it ends up applied per
  branch.
- **Parameterised, with "off" available.** How early it starts and how hard it
  pulls are look choices; off matters because desaturation *hides* residual cast,
  which is right for a print and wrong for a diagnostic render or for judging a
  decode (Part 3 makes the same point about comparing decodes).
- **It must not double up with the gamut map.** The SDR renderer already does
  something like this at the cube boundary as a side effect; this makes it
  deliberate, and the display stages need to know which of the two is acting.

## Open questions

- What "approaches white" is measured on — luminance, max channel, or a
  saturation measure — and whether the pull preserves hue.
- Whether anything is applied above diffuse white per branch, or the operator
  stops there entirely.

## How to Verify

- Off is a bit-exact identity.
- A saturated near-white patch desaturates monotonically as the parameter rises.
- On a real frame, `nctool metrics` reproduces a decile B/R trend in the direction
  Appendix E measured.
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
