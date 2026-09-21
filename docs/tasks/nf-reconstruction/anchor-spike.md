# Spike: does a diffuse-white anchor earn its place?

## Goal

Price a **diffuse-white anchor**, knowing its value depends on a per-channel operator
existing to exploit it and that none ships today. [The anchor rule](anchor-rule.md) has
to choose between a mid anchor and a highlight one, and
[path to white](../nf-look/path-to-white.md) has to know whether it is a remedy for
cast or an optional look. Neither settles alone.

## Design

Two findings narrow this before any render, both recorded in
`docs/reports/three-way-gold200.md`:

- **The anchor cannot converge channels by itself.** It factors out of
  `out_c = 10^(gamma·(scale_c·D_c − A))` as a common gain. Its only job is deciding how
  much content lands where a per-channel operator acts.
- **Three of design-update Appendix F's four confounds die by algebra**, leaving the
  sigmoid's **per-channel shoulder** and the **gamut map**'s radial convergence near
  luminance 1.0. Separating those two is the open question; that per-channel
  compression converges whites is structural and needs no re-demonstration.

The report also settles *where* such an operator belongs — referenced to diffuse white,
pre-branch, because the two branch ceilings differ — and that it does not replace fit
range. This spike does not revisit either.

**It needs none of the migration.** Today's binary carries the anchor family and the
sigmoid's per-channel shoulder, so the candidate is a flag combination:

- **candidate** — `--density-curve sigmoid --sigmoid-shoulder <s>
  --anchor-white-at-reference --d-max <D>`, `D` a **diffuse-white** density.
- **control** — the same curve and shoulder at `--anchor-mid-offset <D>`.
- **separator** — the same pair with `--display-tone none` against `reinhard`, since the
  gamut ceiling follows the rendered luminance; that is what tells the shoulder's share
  from the gamut map's.
- **targets** — `--preset sigmoid-knees` and SFC, the least-cast outside converter.

**Brightness held fixed across the set.** The anchor *is* a gain, so an unmatched
comparison measures exposure — the 2026-09-17 lesson, repeated.

**Per roll, not per frame.** A per-frame anchor is NLP's method and the mechanism behind
its worst failure; it also normalises HDR headroom away.

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
- **What the operator costs**, which the whites comparison does not show: the midtone
  shift, whether **saturated highlights flatten** (a sunset *is* saturated highlights),
  and how much **residual cast it hides** — the property that makes "off" mandatory.
- **Whether the anchor still earns a change once the operator exists.** A strong enough
  operator at a mid anchor may reach far enough down to make the anchor change
  unnecessary.

## How to Verify

**Judge on SDR, measure HDR separately** — the targets were ranked on SDR and the
three-way numbers measured there, so the comparison is SDR with the gain map stripped
(`render-review-set` carries the procedure). HDR enters as the headroom number only.

**Both a measurement and a visual pass.** The original verdict was visual, the metric is
a proxy, and the reviewer is insensitive to light green — so green is read off marked
neutral patches (the review app measures them natively since #128) and the rest ranked
by eye.

A written answer in `docs/progress/nf-reconstruction.md`, against the control on the
same frames: **whites** (top-end chroma on the patches), **shoulder versus gamut map**
(which share of the convergence is which), **midtones** (the level shift in stops, and
whether the documented 2.5–3.6 reproduces once the reference is diffuse white rather
than a leader `Dmax`), **headroom** (the resulting `GainMapMax`), and **the user's
ranking** against `sigmoid-knees`.

Any of the three outcomes — it works, it works but costs too much, it does not work —
completes the spike.

## Dependencies

None — it runs against today's binary, deliberately, so that it can run before the rule
has to be chosen.
