# One gamut-mapping implementation

## Goal

Move out-of-gamut colour to the destination's boundary with one implementation
instead of three. Today `sdr`, `hdr` and `gain_map` each carry a near-copy of the
same neutral-axis radial map, differing mainly in the ceiling they pass it.

## Design

What is known:

- **Three copies, one algorithm.** `src/pipeline/sdr.rs:289`,
  `src/pipeline/hdr.rs:603` and `src/pipeline/gain_map.rs:527` are all
  `neutral-axis-radial-boundary-v1`; the divergence is the ceiling, and the
  ceilings differ for good reasons — SDR's follows what fit range produced,
  HDR's is the linear headroom, and the gain map's must match the base *as
  stored*. A single function takes the ceiling as an argument; it does not
  decide it.
- **Fit range and fit gamut stay adjacent but separate** (design-update Part 2):
  the ceiling is an output of the previous stage, which is exactly why they were
  fused and exactly why they should not be.
- **The gain-map ceiling is the load-bearing one.** CLAUDE.md records what
  ratioing against the rendered SDR rather than the stored one cost — so unifying
  the implementation must not unify the ceiling.
- **Written fresh**, and the destination gamut is a parameter: Adobe RGB is a
  must-have output under the new flow, so a third `SdrGamut`-shaped arm should not
  require a fourth copy.

Open:

- **Does the look's highlight desaturation subsume part of this?** Design-update
  Part 2 notes the SDR gamut map already pulls chroma out near white as a side
  effect; once the look does it deliberately, the boundary behaviour may want to be
  less aggressive. Decide which operator owns the path to white.
- Whether hue preservation is a stated contract with a test, or an emergent
  property of the radial form, as it is today.

## How to Verify

- One function; the three call sites pass their own ceiling and nothing else
  differs. A test pins that an SDR and a gain-map render of one frame still
  disagree only where the ceilings do.
- Out-of-gamut vectors stay finite, reach the boundary, and keep hue — coverage
  written against the new stage, not carried over.

## Dependencies

- [Fit range as one stage](fit-range.md) — the ceiling this stage consumes is what
  fit range produces
