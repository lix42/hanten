# One gamut-mapping implementation

## Goal

Move out-of-gamut colour to the destination's boundary with one implementation
instead of three. When filed, `sdr`, `hdr` and `gain_map` each carried a near-copy of
the same neutral-axis radial map, differing mainly in the ceiling they passed it (now
one function — see Outcome).

## Design

What is known:

- **Three copies, one algorithm** (as filed): `sdr`, `hdr` and `gain_map` were all
  `neutral-axis-radial-boundary-v1`; the divergence is the ceiling, and the
  ceilings differ for good reasons — SDR's follows what fit range produced,
  HDR's is the linear headroom, and the gain map's must match the base *as
  stored*. A single function takes the ceiling as an argument; it does not
  decide it.
- **Fit range and fit gamut stay adjacent but separate** (design-update Part 2):
  the ceiling is an output of the previous stage, which is exactly why they were
  fused and exactly why they should not be.
- **The gain-map ceiling is the load-bearing one.** `gain_map::build`'s docs record what
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
- ~~Whether hue preservation is a stated contract with a test~~ — **answered
  2026-09-24**: it is, in linear destination RGB (see Outcome).
- ~~The peak is not on fit range's boundary yet~~ — **answered 2026-09-24**:
  `RangeFittedImage` carries it.

## Outcome

**Done 2026-09-24.** Trail in `docs/progress/nf-display-stages.md`.

- One primitive, `fit_gamut::radial_to_boundary(rgb, luminance, ceiling)`; the four
  legacy call sites (SDR, HDR display, HLG's scene-linear pass, the gain map) pass
  their own ceilings and render byte-identically.
- The new stage's ceiling is `max(peak, Y)`, with the peak carried on
  `RangeFittedImage`: above the peak the cube holds only the neutral, so such content
  desaturates to white and clips, counted, at the encode. `Y ≤ 0` renders black.
- Hue preservation is a tested contract (one common scale about neutral in linear
  destination RGB), not a claim about perceptual hue.
- Report id `acescg-to-display-p3-matrix+neutral-axis-radial-boundary-v2`.
- **How the first criterion was met.** "SDR and gain-map renders disagree only where
  the ceilings do" is verified by the current chain rendering **byte-identically**
  before and after the switch (36 of 36 preset × tone × fixture renders) with every
  legacy golden unchanged — so each caller still passes the ceiling it did. No new
  render-level test was added. The primitive's behaviour at each caller's ceiling is
  pinned bit for bit (`golden_the_shared_map_is_bit_identical_at_every_callers_ceiling`),
  since the current chain now renders through a new-flow module.
- The HDR half of the "does the look subsume it" question is still unmeasured: no HDR
  destination exists under the new flow yet.

## How to Verify

- One function; the three call sites pass their own ceiling and nothing else
  differs. A test pins that an SDR and a gain-map render of one frame still
  disagree only where the ceilings do.
- Out-of-gamut vectors stay finite, reach the boundary, and keep hue — coverage
  written against the new stage, not carried over.

## Dependencies

- [Fit range as one stage](fit-range.md) — the ceiling this stage consumes is what
  fit range produces
