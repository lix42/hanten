# Spike: does a diffuse-white anchor earn its place?

## Goal

Answer, against today's binary, whether anchoring at **diffuse white** produces the
neutral whites the three outside converters get, what it costs in the midtones, and
how much HDR headroom it leaves. [The anchor rule](anchor-rule.md) currently has to
choose between a mid anchor and a highlight one on argument alone.

## Design

`docs/reports/three-way-gold200.md` established that all three converters anchor the
bright end (96–97th percentile of the frame's own density) and that this — not any
colour step — is why their whites read neutral. nc's chosen rule anchors the mid.

**The experiment needs none of the migration.** Today's binary already carries the
whole anchor family, so the candidate is a flag combination, not code:

- `--anchor-white-at-reference --d-max <D>` pins display white at a chosen density.
  Setting `D` to a **diffuse-white** density is the untested case.
- `--anchor-mid-offset <D>` is the design's chosen rule, as the control.
- `--preset sigmoid-knees` is the render the user has ranked best, as a target;
  SFC is the least-cast outside converter and can sit in the set as a second.

**The known failure is the thing to isolate.** `--anchor-white-at-reference`'s help
text records that it renders midtones 2.5–3.6 stops dark at photographic contrast,
"sensible only when the reference is itself a diffuse white". Every previous use set
the reference from a leader `Dmax`, which sits well above diffuse white — so
everything fell below it. Whether the cost survives when the reference *is* diffuse
white is exactly what is unmeasured.

**Per roll, not per frame.** A per-frame anchor is NLP's method and the mechanism
behind its worst failure; it also normalises HDR headroom away. Derive one density
per roll from a high percentile across the roll's frames, with a gap below the
leader's `Dmax` so a single blown frame cannot drag it.

## Open questions

- **Which percentile, and of what.** Per-frame p96–97 is what the converters use, but
  nc needs a roll-level statistic, and pooling frames is not the same thing.
- **How to derive diffuse white at all** on a roll with no white surface in it.
- Whether the midtone cost, if real, is answerable by the look stage's print contrast
  rather than by rejecting the anchor.

## How to Verify

A written answer in `docs/progress/nf-reconstruction.md` covering three measurements,
against the mid-anchored control on the same frames:

- **Whites** — top-end chroma on marked neutral patches, by the method in the
  three-way report (green is measured, not judged).
- **Midtones** — the level shift, in stops, and whether the 2.5–3.6 figure reproduces.
- **Headroom** — the resulting `GainMapMax`, against the ~1 stop the density
  measurements predict and the 1.0× the shipped default delivers.

Plus the user's ranking against `sigmoid-knees`. Any of the three outcomes — it works,
it works but costs too much, it does not work — completes the spike.

## Dependencies

None — it runs against today's binary, deliberately, so that it can run before the
rule has to be chosen.
