# Spike: does a diffuse-white anchor earn its place?

## Goal

Price the candidate ways of placing the decode's white, from the scans and arithmetic
alone, and hand [the anchor rule](anchor-rule.md) a costed shortlist instead of an
argument.

**The rendered verdict is deliberately not here.** Under the base-referenced anchor
nothing reaches the region a per-channel operator acts in, so a render before that
operator exists would compare configurations of which one is inert. It is
[`nf-calibration/anchor-comparison`](../nf-calibration/anchor-comparison.md).

## Design

Two things reframed the question before any measurement.

- **The anchor cannot converge channels.** It factors out of
  `out_c = 10^(gamma·(scale_c·D_c − A))` as a common gain, so its only job is deciding
  how much content lands where a per-channel operator acts.
- **"Mid anchor" and "white anchor" name one rule** on a straight line, and nc's current
  value already sits at the datasheets' diffuse white. So the question is not where on
  the scale to pin, but **what the pin is referenced to** — the film base plus a
  constant, or the roll's own content.

Per **roll**, never per frame: a per-frame anchor is NLP's method and the mechanism
behind its worst failure, and it normalises HDR headroom away with the exposure.

Everything measured, and the four candidates it produced, is in
[`docs/reports/white-placement.md`](../../reports/white-placement.md).

## Open questions

Carried forward to [the comparison](../nf-calibration/anchor-comparison.md):

- **Which percentile defines the roll's white**, and of what.
- **How to derive it** on a roll with no white surface in it.
- **Whether the anchor earns a change at all once the operator exists** — a strong
  enough operator at today's anchor may reach far enough down that a roll anchor buys
  nothing, which is why [the operator spike](../nf-look/desaturation-spike.md) should
  report first if the two are not run together.

## How to Verify

A written answer in `docs/progress/nf-reconstruction.md` and a report carrying the
numbers: where each roll's white sits against the base-referenced anchor in the anchor's
own units, on at least three rolls across stocks; what a fixed `d` costs as a spread
between them; and the contrast each candidate demands, against the converters' measured
slopes.

Complete when the shortlist is costed — not when one is chosen.

## Dependencies

None — it runs on the scans, deliberately, so that it can run before the rule has to be
chosen.
