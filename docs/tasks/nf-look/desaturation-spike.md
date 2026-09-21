# Spike: what form should highlight desaturation take?

## Goal

Settle the form of the operator [path to white](path-to-white.md) would ship, and which
mechanism it has to reproduce. The design says chroma goes to zero as a pixel approaches
diffuse white; it does not say *measured on what*, and the two plausible answers produce
visibly different pictures.

## Design

`docs/reports/three-way-gold200.md` settled the parts that are structural, and this
spike does not revisit them: that per-channel compression against a common ceiling is
what converges channels, that no nc display tone can do it (all three are
luminance-preserving), and that the operator belongs pre-branch referenced to diffuse
white rather than in fit range, whose ceilings differ per branch.

What is left is the form, and one loose end.

**Two candidate families, and nothing has compared them.**

- **A per-channel curve** — each channel compressed against a common ceiling, which is
  what film, paper and all three outside converters do. It desaturates *and shifts hue*.
- **A hue-preserving chroma pull** — chroma scaled toward the neutral axis as a function
  of approach to diffuse white, hue held.

The design's prose describes the second; every measured reference does the first. If the
first matches the knee'd render and the second does not, the **hue shift is part of what
the eye was responding to** — which changes what the operator is, not just how it is
tuned.

**The loose end: shoulder or gamut map?** Three of design-update Appendix F's four
confounds die by algebra, leaving two — the sigmoid's per-channel shoulder, and the
**gamut map**'s radial convergence near luminance 1.0 (`sdr.rs:249-266`), whose ceiling
follows the rendered luminance. Both are per-channel-ish, so the mechanism is real
either way, but their shares decide how much work the new operator has to do and whether
it would double up with the gamut map. `--display-tone none` against `reinhard` moves
that ceiling and is the cheapest separator.

**Reachable today.** The per-channel family needs no new code — the sigmoid's shoulder
*is* one, reachable as `--sigmoid-shoulder <s>`. The hue-preserving family does not
exist and needs a throwaway patch in `render_split::display_source`, which is where the
shipped one would eventually sit. Both compare against `--preset sigmoid-knees` and SFC.

## Open questions

- **What "approaches white" is measured on** — luminance, the max channel, or distance
  from the neutral axis. The three disagree exactly where saturated highlights live.
- **What it costs on saturated colour.** A sunset *is* saturated highlights, so whatever
  cleans whites also dulls them. This is the measurement that decides whether the default
  is on or off, and it needs a sunset frame in the set deliberately.
- **How much residual cast it hides** — the property that makes "off" mandatory and makes
  it wrong for the `direct` preset, and the reason it cannot also be the remedy for cast.
- Whether anything is applied above diffuse white per branch, or the operator stops there.

## How to Verify

A written answer in `docs/progress/nf-look.md` covering:

- **Whites** — top-end chroma on marked neutral patches (the review app measures them
  natively since #128), per family and strength, against `sigmoid-knees` and SFC.
- **Which family the eye prefers**, ranked on frames that include at least one sunset or
  strongly-coloured highlight, not only white surfaces.
- **The gamut map's share** of the convergence, from the `--display-tone` pair.
- **The cost** — saturation loss on those coloured highlights, and how much a known cast
  is suppressed (render a frame with a deliberately wrong `density.scale` and measure how
  much of it survives).

Brightness held fixed across the set, SDR with the gain map stripped, and green read off
patches rather than judged — the reviewer is insensitive to light green.

Any outcome completes it, including "neither family reproduces it", which would send the
question back to the anchor.

## Dependencies

None — the per-channel half runs against today's binary and the hue-preserving half is a
throwaway patch, so it can run before [the look stage](stage.md) exists.
