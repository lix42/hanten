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

What converges channels is the **shoulder applied per channel against a common
ceiling**: each channel asymptotes to the same 1.0, so one arriving higher is
compressed more and the gap shrinks as brightness rises (SF measured 1.13× apart at
p97, 1.05× at p99.5). The anchor's job is only to decide how much content lands in
that compressed region. *Per channel* here means **one shoulder parameter evaluated on
each channel's own density** — `s_curve` through `density::apply_curve`, which is what
`sigmoid-knees` already does. Nothing sets a different shoulder per channel, and this
task does not propose one.

`reinhard` cannot substitute, and the reason is structural rather than a matter of
strength: `sdr.rs` computes one luminance, curves that, and multiplies all three
channels by the resulting ratio. A common multiplier leaves every channel ratio
invariant, so a cast is carried untouched to display white however hard it compresses.
So the anchor is varied **with the per-channel shoulder present**, or it is varied
against nothing.

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

## The decode-side half

Some of this needs no rendering at all — it is the scans plus arithmetic, and it
should be settled first because it decides what the rendered set is even testing.

**What a white anchor does to the rest of the scale.** `algo/sigmoid.rs` records that
the anchor "sets the black floor at `10^(−contrast·anchor)`", so pinning white *higher*
darkens everything — which is where the documented 2.5–3.6 stops comes from. The pale
risk is the compensation, not the anchor: brighten with `print_exposure` and the black
floor lifts with everything else; raise contrast instead and the black returns, but
contrast and exposure are now coupled. **That coupling is the real cost**, and it is
exactly the property [the rule](anchor-rule.md) credits the mid anchor with. Measure
where mid-grey and black land across a contrast range, per candidate anchor, before
rendering anything.

**Where white actually is.** Measure, on this roll and at least one other:

- the density of the roll's diffuse white under each candidate rule (a high percentile
  pooled across frames, per-frame p96–97, and the leader's `Dmax`);
- the **gap between diffuse white and the leader's `Dmax`** — the headroom a specular
  tail needs, and the margin a single blown frame would otherwise eat;
- how much that gap varies frame to frame and roll to roll, since the anchor is fixed
  per roll but the content is not.

## Open questions

- **Which percentile, and of what.** Per-frame p96–97 is what the converters use, but
  nc needs a roll-level statistic, and pooling frames is not the same thing.
- **How to derive diffuse white at all** on a roll with no white surface in it.
- **How much headroom to leave**, which is the same choice as the percentile: p95/p97/p99
  measured 1.23/1.03/0.67 median stops on `2026-09-18-Gold200`.
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
