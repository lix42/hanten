# Spike: does a diffuse-white anchor earn its place?

## Goal

Price the candidate ways of placing the decode's white, from the scans and arithmetic
alone, and hand [the anchor rule](anchor-rule.md) a costed shortlist instead of an
argument. **The rendered verdict is deliberately not here** — it needs a per-channel
operator to exist before the differences are visible, so it is
[`nf-calibration/anchor-comparison`](../nf-calibration/anchor-comparison.md).

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
## The shortlist

Three ways to place white, with what each gets right. Measured on
2026-09-18-Gold200, whose roll white `W` sits at red density 0.800 against a datasheet
`d` of 0.62 (`docs/progress/nf-reconstruction.md`, 2026-09-21):

| | gamma | anchor `A` | a datasheet mid renders at | white reached |
|---|---|---|---|---|
| **A** fixed anchor — `mid-at-base-offset(d)` | 2.00 | 0.992 | **0.180** ✓ | 0.412 ✗ |
| **B** content white — `A = W`, a level move | 2.00 | 0.800 | 0.437 ✗ | **1.000** ✓ |
| **C** hybrid — pin mid *and* solve contrast | 4.15 | 0.800 | **0.180** ✓ | **1.000** ✓ |

**C is the only one that pins both ends**, and structurally so: it has two free
parameters (anchor and contrast) against two constraints where A and B each have one.
Its contrast follows from `gamma = MID_GREY_OUTPUT_DECADES / (W − d)` — 4.15 / 2.57 /
3.11 on the three rolls measured.

**B is weaker than "level is free" suggested.** A level move puts a true datasheet
mid-grey at 0.437 instead of 0.18, because it assumes the roll's brightest *is* white
when the roll may simply be flat.

**C stays inside the design.** `gamma` already splits — linearization (~1.8) in the
decode, print contrast in rendering — so C's per-roll contrast is the **look stage's
contrast knob** (2.31× / 1.43× / 1.73× over 1.8), not a decode that varies per roll.

**C's cost is noise, and it is not small.** `corr(slope, noise) = 0.89`, and C asks
Gold200 for gamma 4.15 where the converters measured 3.07–3.18 on that same roll —
because its content spans only 0.18 density from `d` to its bright end where the
datasheets expect 0.36. C cannot tell a genuinely flat roll from a wrong `d`, and pays
for either in contrast.

**D — C with a ceiling** is therefore worth carrying: cap gamma at a noise budget, and
when the cap binds, close the remaining gap by sliding `A` down toward `W`, i.e. degrade
toward B rather than refusing. Well-formed, and it makes the noise budget the explicit
parameter it should be.

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

A written answer in `docs/progress/nf-reconstruction.md`, all of it from the scans:

- **Where each roll's white sits** against the base-referenced anchor, in the anchor's
  own units (red, where `scale = 1`), on at least three rolls across stocks.
- **What a fixed `d` costs** — the spread between rolls at the bright end.
- **The contrast each option demands**, against the converters' measured slopes.
- **Enough for the shortlist above to be read without re-deriving it.**

Complete when the shortlist is costed. The rendered comparison is a separate task and
should not be attempted here: under option A nothing reaches the region a per-channel
operator acts in, so a render would compare three configurations of which one is inert.

## Dependencies

None — it runs on the scans, deliberately, so that it can run before the rule has to be
chosen.
