# Highlight desaturation

## Goal

Chroma goes to zero as a pixel approaches white, as a deliberate, parameterised
part of the look — anchored at diffuse white rather than at a branch's display
white.

## Design

- **Why it has to be explicit.** Measured 2026-09-17 (design-update Appendix E):
  the knee'd sigmoid reads clean on every white surface because its *per-channel*
  shoulder pulls the channels together as lightness rises, B/R 1.65 → 1.27 across
  one frame's deciles against 1.76 → 1.48 for the shoulder-less render. A
  luminance-preserving operator scales all three channels by one factor, so a cast
  survives to display white. It is a real print behaviour — paper applies
  per-channel curves — so the look stage owes it explicitly rather than
  inheriting it from a retiring reconstruction curve.
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
- [Spike: can a look-stage operator reproduce the knee'd sigmoid's whites?](path-to-white-spike.md)
