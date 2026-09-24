# Choose the white placement by rendering

## Goal

Pick between the four white-placement options
[the spike](../nf-reconstruction/anchor-spike.md) costed — fixed anchor, content white,
the pin-both-ends hybrid, and the hybrid with a noise ceiling — by rendering them with a
real highlight operator in the chain and judging the result.

## Design

The spike settled what each option does to the *scale*; only a render settles which one
looks right. It deliberately stopped there, because under option **A** nothing reaches
the region a per-channel operator acts in — measured 0.55–1.73 stops short of white on
three rolls (first published as 0.55–1.28; the 09-11 figure included a calibration frame —
see the correction in [`white-placement.md`](../../spike/white-placement.md)) — so a render before the operator exists would compare four configurations
of which one is inert.

The shortlist, from [the report](../../spike/white-placement.md):

| | pins mid | reaches white | per-roll contrast |
|---|---|---|---|
| **A** fixed anchor | ✓ | ✗ | fixed |
| **B** content white, level move | ✗ | ✓ | fixed |
| **C** hybrid, contrast solved | ✓ | ✓ | 1.43–3.67× (09-11 corrected) |
| **D** C with a gamma ceiling, sliding toward B when it binds | ✓ | mostly | capped |

The known failure modes of a content-referenced white (a blown frame, an underexposed
roll) and the measured HDR headroom per anchor percentile are in
[`anchor-rule`'s handoff section](../nf-reconstruction/anchor-rule.md#evidence-handed-to-anchor-comparison).

**What the render has to separate**, and the arithmetic cannot:

- Whether B's washed-out midtone (a datasheet mid at 0.437 rather than 0.18) reads as
  wrong or merely bright.
- Whether C's contrast is visible as *punch* or as *noise* — it asks Gold200 for gamma
  4.15 where the converters measured 3.07–3.18 on that roll, and noise tracks slope at
  r = 0.89.
- Where D's ceiling should sit, which is the same noise budget
  [scene-range mapping](../nf-look/scene-range-mapping.md) needs and should share.
- Whether any of them beats simply moving `d`, which is the null hypothesis.

**Held fixed across the set**: one operator configuration, one rendering otherwise, and
brightness matched where the comparison is about colour rather than tone — though note
that A versus the rest *is* partly a brightness comparison, so it cannot be matched away.

**Per roll, on at least three stocks.** The spread between rolls is the thing being
traded; one roll cannot show it.

## Open questions

- **Which percentile defines `W`.** The spike used red p97 pooled toward the roll's
  upper end; p95 and p99 move the answer and the specular headroom with it.
- **Whether the per-roll contrast is a recipe value or a measurement** the roll planner
  derives — the first is reproducible, the second is convenient. White balance chose the
  first (`nf-scene-correction/roll-white-balance`): `hanten measure-roll` measures once
  and the recipe carries the value. Its pooled measurement could report the roll white's
  *level* as well, from the same pixels and leader guard — share that measurement, not a
  parameter, if this lands on a measured `W`.
- Whether an underexposed roll should be lifted at all, which is the faithfulness
  question the whole shortlist turns on and which no measurement decides.

## How to Verify

A written answer in `docs/progress/nf-calibration.md` recording the ranking, on at least
three rolls, with the measurements beside it: delivered noise per option (the cost),
top-end chroma on marked neutral patches (whether the operator had anything to act on),
and where a datasheet mid-grey actually landed.

A verdict of "A, and move `d` instead" is a complete outcome — the shortlist exists to
be beaten, not adopted.

## Dependencies

- [Highlight desaturation](../nf-look/path-to-white.md) — the operator has to exist, or
  option A has nothing to compare with
- [Spike: does a diffuse-white anchor earn its place?](../nf-reconstruction/anchor-spike.md)
  — supplies the shortlist and its arithmetic
