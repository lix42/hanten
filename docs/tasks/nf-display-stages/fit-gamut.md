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

- **Does the look's highlight desaturation subsume part of this?** Mostly answered
  2026-09-23 by [`gamut-map-share`](gamut-map-share.md): the look owns the path to
  white. Under today's `reinhard` and `none` in SDR the map moved no marked white on
  four rolls, and every limit it hit was the cube's top. It does real work under a tone
  that plateaus near display white (the current `shoulder`, which the new flow retires),
  so the answer holds only if fit range's operator does not plateau there — re-check
  once [the parametric operator](parametric-operator.md) is chosen, and for HDR, which
  was not measured.
- **No diagnostic off switch** (decided 2026-09-23). Should one ever be wanted, "off"
  has to mean an unmapped **float** destination — a render that skips the map and then
  quantizes has swapped the radial map for a per-channel clip, which is a gamut policy
  of its own.
- Whether hue preservation is a stated contract with a test, or an emergent
  property of the radial form, as it is today.

- **The peak is not on fit range's boundary yet** (2026-09-23): `RangeFittedImage`
  carries pixels only. If the ceiling needs the display's peak rather than each
  pixel's luminance, add it there — fit range resolves it.

## How to Verify

- One function; the three call sites pass their own ceiling and nothing else
  differs. A test pins that an SDR and a gain-map render of one frame still
  disagree only where the ceilings do.
- Out-of-gamut vectors stay finite, reach the boundary, and keep hue — coverage
  written against the new stage, not carried over.

## Dependencies

- [Fit range as one stage](fit-range.md) — the ceiling this stage consumes is what
  fit range produces
