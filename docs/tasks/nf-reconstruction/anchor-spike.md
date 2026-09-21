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

**The anchor alone cannot do it, and the experiment has to reflect that.** In
`out_c = 10^(gamma·(scale_c·D_c − A))` the anchor factors out as `10^(−gamma·A)`,
identical on every channel — a pure common gain that cannot move a channel ratio.
What the converters do is anchor high **and** compress above it *per channel*: the
soft-min's compression differs by channel according to where each one sits relative
to the ceiling, and the anchor decides how much content lands in that region. So the
anchor is varied **with a per-channel shoulder present**, or it is varied against
nothing. `reinhard` cannot substitute — it scales all three channels by one factor
and so cannot converge them by construction.

**It still needs none of the migration.** Today's binary carries the whole anchor
family and the sigmoid's per-channel shoulder, so the candidate is a flag
combination, not code:

- **candidate** — `--density-curve sigmoid --sigmoid-shoulder <s>
  --anchor-white-at-reference --d-max <D>`, with `D` a **diffuse-white** density.
  That is the untested case.
- **control** — the same curve and shoulder at `--anchor-mid-offset <D>`, so the only
  thing that moves is the anchor.
- **targets** — `--preset sigmoid-knees`, the render ranked best so far, and SFC, the
  least-cast outside converter.

**Brightness must be held fixed across the set.** The anchor *is* a gain, so an
unmatched comparison measures exposure and not the anchor's structural effect.
Match on a measured statistic with `--print-exposure` and verify it before judging
anything — the 2026-09-17 round's lesson, repeated.

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

**Rendered results, not reconstruction output.** The effect only exists downstream of
a per-channel nonlinearity, and design-update Part 3 rules out judging a decode
against a rendered target anyway: any invertible reconstruction can be compensated by
some rendering, so the comparison has to be of finished images with the rendering
held fixed.

**Judge on SDR, measure HDR separately.** The targets were ranked on SDR and the
three-way numbers were measured there, so the whites-and-midtones comparison is SDR
with the gain map stripped (`render-review-set` carries the procedure). HDR enters
only as the headroom measurement, which is a number rather than a judgement.

**Both a measurement and a visual pass**, because neither settles it alone: the
original verdict was visual, the metric is a proxy for it, and the reviewer is
insensitive to light green. So green is read off marked neutral patches and the rest
is ranked by eye.

A written answer in `docs/progress/nf-reconstruction.md` covering, against the
control on the same frames:

- **Whites** — top-end chroma on marked neutral patches, by the three-way report's
  method.
- **Midtones** — the level shift in stops, and whether the documented 2.5–3.6 figure
  reproduces once the reference is diffuse white rather than a leader `Dmax`.
- **Headroom** — the resulting `GainMapMax`, against the ~1 stop the density
  measurements predict and the 1.0× the shipped default delivers.
- **The user's ranking** against `sigmoid-knees`.

Any of the three outcomes — it works, it works but costs too much, it does not work —
completes the spike.

## Dependencies

None — it runs against today's binary, deliberately, so that it can run before the
rule has to be chosen.
