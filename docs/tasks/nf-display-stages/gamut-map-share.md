# Separate the gamut map's share of highlight desaturation

## Goal

Find out how much of the highlight chroma convergence nc already produces comes from
the **gamut map** rather than from a tone or reconstruction curve — and decide whether
the gamut map needs a way to be turned off for diagnosis.

## Why it is its own task

Two live questions rest on the answer and neither can settle without it:

- [The path to white](../nf-look/path-to-white.md) must not double up with the gamut
  map. Every desaturation measurement taken so far reads **shoulder-plus-gamut-map
  jointly**, so an unknown share of the cleanup being credited to the operator may
  already be happening.
- The knee'd sigmoid's clean whites have **two** surviving candidates after
  [Appendix F](../../design-update.md) eliminated the other three: the per-channel
  shoulder and the gamut map, which converges radially near luminance 1.0 with a
  ceiling that follows the rendered luminance (`sdr.rs:249-266`) — so the display tone
  reaches it *indirectly* even though no nc tone can converge channels itself.

It runs against **today's binary**, so it does not wait for the new chain.

## Design

What is known:

- **Nothing reachable by flag turns the gamut map off**, which is why the spike could
  not separate them.
- **`--display-tone shoulder` failed as a separator** and the failure is informative: it
  drives top-end chroma to exactly 0.0 with 17–29° of hue rotation. That is flattening,
  not shaping — it plateaus above its knee and leaves the map no room for chroma at all.
- Two routes, and they answer slightly different questions. A **throwaway patch**
  disabling the map measures its share directly on real frames. **Counting top-end
  samples out of gamut before mapping** needs no patch and says how much material the
  map has to act on, which bounds the share without measuring it.

## Open questions

- **Which route**, or both — the count is cheap and bounds the answer; the patch is the
  measurement.
- **Does the gamut map earn a diagnostic off switch** as a shipped flag? A render that
  hides residual cast is wrong for a diagnostic, which is the same argument that gives
  `path-to-white` its "off". Against it: an unmapped render is out of gamut by
  construction, so "off" would have to mean something precise.
- **Does this change what the map should do**, or only what is measured? If the map is
  carrying a large share, the design's "what should shrink instead is the gamut map's
  incidental share" becomes a real instruction to
  [fit gamut](fit-gamut.md) rather than a remark.

## How to Verify

**Done 2026-09-23.** Result: [`docs/reports/gamut-map-share.md`](../../reports/gamut-map-share.md),
trail in `docs/progress/nf-display-stages.md`. Measured by a throwaway in-crate probe
reading both sides of the map in float (so "off" was unmapped, not clipped), bit-identical
to `sdr::render` on every accepted render. The share is near zero where it matters, the
knee'd whites are the shoulder's, and the decision is no off switch on either chain.
The first criterion below is met in a changed form: the report gives the **absolute**
C\* the map removes per roll rather than a share, because outside `sigmoid-knees` the map
is the only thing converging chroma (a share would be 100% or 0/0), and inside it the
2x2's other ordering needs a render the shipped path refuses.

- A number, per roll: the share of top-end chroma convergence attributable to the gamut
  map, with the measurement's own method stated (patch or count).
- The two candidates for the knee'd render's whites are separated, or the reason they
  cannot be is recorded.
- A recorded decision on the diagnostic off switch.

## Dependencies

- [Spike: what form should highlight desaturation take?](../nf-look/desaturation-spike.md)
  — **done 2026-09-21.** Raised this as its one unseparated term.

Depended on by [the path to white](../nf-look/path-to-white.md).
