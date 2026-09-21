# Spike: does a diffuse-white anchor earn its place?

## Goal

Decide whether the decode's white should be **derived from the roll's own content**
instead of from the film base plus a constant. [The anchor rule](anchor-rule.md) treats
that as settled by principle; the measurements now give it a price.

## Design

**The mid-versus-white framing is a false one, and that is what leaves the real
question.** `AnchorPlacement::anchor` resolves `mid-at-base-offset(d)` to
`d + MID_GREY_OUTPUT_DECADES / contrast`, and that value *is* the density where output
reaches 1.0. At `d = 0.62`, `contrast = 2.0` it is **0.9924** — against the datasheets'
diffuse white at `d + REFERENCE_MID_TO_WHITE_DELTA` = **0.98**, a gap of 0.012 density,
about 0.08 stops. **nc already anchors at diffuse white**, spelled as a mid anchor, and
on a straight line the two names describe one rule.

So the choice is not where on the scale to pin, but **what the pin is referenced to**:

- **Base-referenced** (today) — white sits a fixed density above the film base. It is
  *open-loop with respect to content*: it says where white would be if the roll were
  exposed to the aim, and never checks whether it got there. An underexposed roll tops
  out below 1.0; an overexposed one runs past it.
- **Content-referenced, per roll** (the candidate) — white pinned to a high percentile
  of the roll's own density. Guarantees the roll's brightest content reaches the region
  a per-channel operator acts in, which is the only reason the anchor matters at all
  (it cannot converge channels itself — it factors out as a common gain).

Per **roll**, never per frame: a per-frame anchor is NLP's method and the mechanism
behind its worst failure, and it normalises HDR headroom away along with the exposure.

**Contrast is the other lever on the same thing, and the two anchors trade which end
floats.** Under a base anchor `A = d + 0.7447/contrast`, so raising contrast pulls white
*down* toward the content — which is how the converters get a flat negative up to white
without moving their anchor.

| | pins | floats with contrast |
|---|---|---|
| `mid-at-base-offset` | mid-grey | white |
| `white-at-reference` | white | mid-grey |

Neither pins both. That is the documented 2.5–3.6 stops seen from the other side, and it
is the cost this spike has to price.
**It needs none of the migration.** `--anchor-white-at-reference` already resolves
`A = reference`, so what is new is only how the reference is computed — offline, then
passed as `--d-max`:

- **candidate** — `--anchor-white-at-reference --d-max <roll content percentile>`,
  with a per-channel shoulder present so the anchor has something to act through.
- **control** — the same curve and shoulder at `--anchor-mid-offset <d>`, i.e. today's
  base-referenced rule.
- **targets** — `--preset sigmoid-knees` and SFC, the least-cast outside converter.

**Two guards on the percentile**, and they pull opposite ways: a **high percentile
rather than the maximum**, or one blown frame drags the anchor up and darkens the whole
roll; and a margin that leaves the measured **~1 stop of specular headroom** above white
as well as distance below the leader's `Dmax`.

**Brightness held fixed across the set.** The anchor *is* a gain, so an unmatched
comparison measures exposure — the 2026-09-17 lesson, repeated.

**The cost to weigh is philosophical before it is physical.** A roll-content anchor
normalises roll-level exposure, so two rolls of one scene at different exposures render
alike — which is "show what the negative really holds" giving ground. What it does *not*
cost is noise: the three-way measurements put the correlation between how far a frame was
lifted and its delivered noise at **−0.49**, so level is free and only *stretching*
grains an image up. A flat roll still pays, because reaching white there needs contrast,
not level.

## The decode-side half

Settle first, from the scans and arithmetic alone, because it decides what the rendered
set is testing.

- **What a white anchor does to the rest of the scale.** The anchor sets the black floor
  at `10^(−contrast·anchor)` (`algo/sigmoid.rs`), so pinning white higher darkens
  everything. Brighten with `print_exposure` and the floor lifts too; raise contrast
  instead and black returns, but contrast and exposure are now coupled — **that coupling
  is the real cost**, and it is the property [the rule](anchor-rule.md) credits the mid
  anchor with. Measure where mid-grey and black land across a contrast range, per anchor.
- **Where white actually is**, on this roll and one other: the diffuse-white density
  under each candidate rule, its gap to the leader's `Dmax`, and how much that gap moves
  frame to frame.

## Open questions

- **Which percentile, and of what** — per-frame p96–97 is what the converters use, but
  nc needs a roll-level statistic, and pooling frames is not the same thing.
- **How to derive diffuse white** on a roll with no white surface in it.
- **Whether the anchor earns a change at all once the operator exists.** A strong enough
  operator at today's base anchor may reach far enough down that the roll anchor buys
  nothing — which is why [the operator spike](../nf-look/desaturation-spike.md) should
  report first if the two are not run together.

## How to Verify

**Judge on SDR, measure HDR separately** — the targets were ranked on SDR and the
three-way numbers measured there, so the comparison is SDR with the gain map stripped
(`render-review-set` carries the procedure). HDR enters as the headroom number only.

**Both a measurement and a visual pass.** The original verdict was visual, the metric is
a proxy, and the reviewer is insensitive to light green — so green is read off marked
neutral patches (the review app measures them natively since #128) and the rest ranked
by eye.

A written answer in `docs/progress/nf-reconstruction.md`, against the control on the
same frames: **whites** (top-end chroma on marked neutral patches — does pinning white
to content put enough of it where the operator acts?), **midtones** (the level shift in
stops, and whether the documented 2.5–3.6 reproduces once the reference is the roll's
own white rather than a leader `Dmax`), **headroom** (the resulting `GainMapMax` against
the ~1 stop predicted), **roll consistency** (that frames keep their relative exposure —
the property a per-frame anchor loses), and **the user's ranking** against
`sigmoid-knees`.

Any of the three outcomes — it works, it works but costs too much, it does not work —
completes the spike.

## Dependencies

None — it runs against today's binary, deliberately, so that it can run before the rule
has to be chosen.
