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
  the gain-map defect CLAUDE.md records was an agreement failure every counter read
  as zero.

Open:

- **Does a destination that renders only one branch still go through the branch
  point?** A single-rendition destination (an SDR TIFF, an HDR AVIF) has no partner
  to agree with, and forcing both renditions costs memory for nothing — but a
  branch that is sometimes skipped is a second code path.
- **What the contract says about the gamut map**, whose ceiling differs per branch
  by design (see [fit gamut](fit-gamut.md)): that is below-white-safe only if the
  ceiling never bites below diffuse white. Confirm rather than assume.
- Whether the contract is a doc statement, a type (one shared source handed to both
  branches), or both.

- **Fit range already gives exact agreement below diffuse white** (2026-09-23): its
  lift is zero there and the base is shared, so the two peaks render the same bits.
  What is open is **above**: the operator keeps reinhard's tail, so HDR content past
  the headroom exceeds `P` (≈1.006·P at `W`, more beyond) and is clamped at the
  encode, where legacy HDR stayed strictly under its 1000-nit peak by dropping the
  tail — at the cost of the below-white agreement. Decide whether an HDR destination
  needs the hard ceiling.

## How to Verify

- A test renders both branches from one source and asserts they agree below diffuse
  white to a stated tolerance, on real frames as well as synthetic ones — and it is
  falsifiable, i.e. it fails when a pre-branch operator is moved below the split.
- A gain map built from that pair reconstructs the HDR rendition; a flat map is
  reported as flat rather than passing silently.

## Dependencies

- [Fit range as one stage](fit-range.md)
- [One gamut-mapping implementation](fit-gamut.md)
