# The SDR/HDR branch contract

## Goal

State where the chain splits into an SDR and an HDR rendition, and what each side
is allowed to differ in. The rule that everything else follows from: a gain map
requires the two renditions to agree below diffuse white.

## Design

What is known:

- **The branch is after the look and before fit range** (design-update Part 2): any
  stage shaping contrast or colour sits above it, or the midtones disagree and the
  gain map breaks.
- **Only fit range and later may differ**, in one argument: the display's peak.
  That is the whole permitted divergence — not two renderers that happen to
  resemble each other, which is what the current code is.
- **Diffuse white is the crossover, and it is scene-referred.** "Display white"
  differs between the branches, so a pre-branch operator cannot be defined against
  it — which is why the look's highlight desaturation is anchored at diffuse white
  too, and the HDR lift already uses that crossover.
- **The agreement is checkable, not merely intended.** Nothing asserts it today;
  the gain-map defect `gain_map::build` records was an agreement failure every counter read
  as zero.

Answered (2026-09-24; the contract is `pipeline::chain`'s module doc):

- **A single-rendition destination goes through the branch point**: it renders one
  branch through the same function as a pair, so skipping a branch skips a call, not a
  code path. A pair costs one full-frame copy of the graded image.
- **The contract is a type and a test.** The headroom is shared with the stages above
  the split (it shapes the midtones), and a branch states only its peak and gamut; a
  pair shares its gamut. The test renders a pair and checks it pixel by pixel.
- **The gamut map is the one permitted difference below white** (user decision
  2026-09-24): where the SDR cube's top binds a saturated colour, the renditions differ,
  and the gain map carries it per channel. The SDR ceiling is `max(1, Y)` in the
  *destination's* luminance, which can exceed 1 while the ACEScg luminance does not.
  Measured on 92 real frames: 0.01% of below-white pixels at the default headroom,
  0.2% at zero; every other below-white pixel bit-identical.

- **An HDR destination gets no hard ceiling above `W`** (user decision 2026-09-24).
  At a 1000/203 peak, 92 frames, no sample exceeds the peak at the default six stops;
  57 samples on 4 frames at three stops; 0.0014% of samples on 21 frames (max 2.01·P)
  at two; 393 on 9 frames (max 3.05·P) at zero. The encoder clamps and counts them —
  for a gain map that is the destination's job, since `gain_ratio::between` clamps HDR
  only to `≥ 0` (recorded in `nf-destinations/preset-set`).

## How to Verify

- A test renders both branches from one source and asserts they agree below diffuse
  white to a stated tolerance, on real frames as well as synthetic ones — and it is
  falsifiable, i.e. it fails when a pre-branch operator is moved below the split.
- A gain map built from that pair reconstructs the HDR rendition; a flat map is
  reported as flat rather than passing silently.

## Dependencies

- [Fit range as one stage](fit-range.md)
- [One gamut-mapping implementation](fit-gamut.md)
