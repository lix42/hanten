# One anchor rule, with a value for `d`

## Goal

`mid-at-base-offset(d)` becomes the only anchor rule on the new flow, with `d` a
fixed runtime constant (≈0.62) carrying a written justification. The four
`--anchor-*` placements collapse to one number, and the decode stops depending on a
reference density measured from a leader.

## Design

The rule pins mid-grey at a fixed density above the film base. Three properties earn
it the job (design-update Part 1, Decisions):

- **Reference-free** — it never reads `Dmax`, so a leader measurement's roll-to-roll
  error cannot reach the render.
- **Contrast and exposure become independent**: changing contrast pivots about mid
  instead of moving the whole image. Pinning black would leave midtone brightness
  depending on contrast, and the base is fog, not scene black.
- **`d` is a calibration, not a brightness knob** — on a straight line the anchor is a
  pure gain, the same lever as exposure. Brightness is rendering's.

**`d` is a convention, not a per-stock value.** Stocks measure 0.542–0.699, ≈1.06
stops at gamma 2; choosing per stock would be per-stock exposure normalization inside
a decode declared stock-agnostic. A fixed value lets film speed show through, which
is the faithful behaviour.

**The known problem is where the number comes from.** The per-stock figures exist
only in a `#[cfg(test)]` table (`STOCK_MID_ABOVE_BASE`, `src/algo/film_stock/mod.rs`), and
the datasheet `d_min` in `film_stock/curves.rs` is documented diagnostic-only — no
render path reads it (`film-stock-profiles` Constraint 1). So this task must
**decide**: lift that prohibition, or hand-freeze one constant with its derivation
recorded beside it. Hand-freezing keeps the constraint intact; lifting it makes a
datasheet a runtime input for a value that must not vary per stock. Resolve that
tension rather than finesse it.

**If the rule ends up reading content, the rule and the measurement split (agreed
2026-09-22).** Every candidate other than the fixed one needs the roll's own bright end
([`docs/spike/white-placement.md`](../../spike/white-placement.md)), and nc already has
the pattern for that: the **rule** is a look knob and the **measurement** it consumes is
a calibration. That is `AnchorPlacement` today — design-spec §8 keeps `curve.anchor` in
the curve precisely because "the anchor is the rule for what the reference places", while
only the measured value leaves. So a content-referenced rule does not move the rule into
`calibration`; it adds a **third measured key** beside `film_base` and `dmax`.
`core/calibration-recipe-section` has been asked to keep that section open rather than
model it as a closed pair. Four consequences:

- **Record what was measured, not a distilled "white".** The headroom table below shows
  p95 / p97 / p99 medians spanning 1.23 → 0.67 on one roll — more than half a stop — so a
  bare scalar loses the definition. Store the percentile with its value — and probably the
  per-frame spread,
  since telling a genuinely flat roll from a wrong `d` is the whole reason candidate D
  exists, and candidate C provably cannot do it.
- **Nothing measures it today.** `estimate` reads one frame; a roll-level percentile
  means reading the roll, with provenance and confidence. That is
  `core/base-acquisition-planner`'s cascade, not a field — so a content-referenced rule
  is a materially bigger change than a reference-free one, and that cost belongs in the
  comparison.
- **It is content-derived, not reference-derived.** `film_base` comes from the unexposed
  rebate and `dmax` from the light-struck leader; a roll white comes from pictures. That
  is a weaker kind of measurement and it is what the two guards below exist for.
- **Keep the placement an enum** whatever this task picks.
  [`nf-calibration/anchor-comparison`](../nf-calibration/anchor-comparison.md)
  has to *render* the candidates to rank them, so collapsing to one rule before that
  comparison runs would remove the thing it measures. Collapsing afterwards is a
  retirement, not a decision to pre-empt here.

**The candidates are cheaper to compare than they look, and the reason says what they
really cost.** `mid-at-base-offset` resolves `A = d + MID_GREY_OUTPUT_DECADES / gamma`,
so the placement is a two-parameter family `(d, gamma)` and every candidate is a point in
it — solve either way. The hybrid fixes `d` and solves `gamma = M / (W − d)` to reach
`A = W`; the level move fixes `gamma` and solves `d = W − M/gamma`, which at gamma 2.0 on
Gold200's `W = 0.800` is 0.4276 — and `white-placement.md`'s own table already prints
`d = 0.428 / 0.538 / 0.488` for the three rolls. So **all four are reachable on today's
binary** as `--density-curve exponential --anchor-mid-offset <d> --density-gamma <g>`,
which is also how
[`nf-look/path-to-white`](../nf-look/path-to-white.md) is being built while this task is
open, and why `--density-gamma` must stay reachable on the new flow.

**Why the exponential rather than the sigmoid, stated properly.** It is *not* that `merge`
refuses `--density-gamma` beside a resolved sigmoid — that rule is real, but you only need
`--density-gamma` because you chose the exponential, and `--density-curve sigmoid
--anchor-mid-offset <d> --sigmoid-contrast <g>` resolves the identical `A`: both curves
carry the same `AnchorPlacement` (`DensityCurve::anchor`), and `--sigmoid-contrast` is
documented as the `--density-gamma` analogue. The real reason is that `A` is the density
mapping to white **on the straight line**, so on a knee'd curve the shoulder compresses
above it and `A = W` does not put rendered white at 1.0 — the same extrapolation caveat
`BlackAtBase`'s rustdoc already records for that curve. Worth knowing, because it means a
highlight operator tuned only under the exponential is tuned on a render with **no
per-channel shoulder** — one of the only two surviving candidates for the knee'd render's
clean whites (design-update Appendix F). Whether the band must also be checked under a
knee'd render is [`nf-display-stages/gamut-map-share`](../nf-display-stages/gamut-map-share.md)'s
to inform.

**So B/C/D need no decode change — they need a measurement of `W` and somewhere to put
it.** The placement only grows a variant if it is to *consume* that measurement rather
than take a hand-set number, which is the same distinction `film_base::estimate` draws by
taking a **resolved** `&FilmBaseSource` rather than the params object. The shape to
prefer, agreed with the `nf-reconstruction/fixed-decode` work: an enum whose **variant
carries its own value**, rather than a positional `reference` that most rules ignore —
today's `AnchorPlacement::anchor(reference, contrast)` ignores that argument in two of
four variants, which is precisely what forced `reads_reference()` to exist as a separate
predicate. A content-referenced variant should carry the measured density.

## Outcome (2026-09-22)

Most of this task shipped with [the fixed decode](fixed-decode.md): one variant, reference-free,
the pure-gain test, and the other three placements refused under `--new-flow` (inventoried
since by [the audit](../nf-core/knob-availability-audit.md)). What was left was the number's origin
and the open questions, answered here; the reasoning is in `docs/progress/nf-reconstruction.md`.

- **The value: hand-frozen, not lifted.** `d = 0.62` is `generic-c41`'s mid aim (0.624
  above base, red), rounded. No render path reads a datasheet, so `film-stock-profiles`
  Constraint 1 stands unchanged. A test in `algo::film_stock` ties the constant to that
  aim and to the stocks' 0.542–0.699 range.
- **`d` stays user-reachable** as `--anchor-mid-offset` and `reconstruction.anchor`:
  every knob is a flag, and `nf-look/path-to-white` is tuned through it.
- **The other three placements** are refused under `--new-flow` and leave the tree with
  the legacy path (`nf-retire/dmax-machinery`).
- **A highlight anchor instead of a mid one** is not a separate question: on a straight
  line the two are one rule (`docs/spike/white-placement.md`). What the anchor is
  *referenced to* — the fixed convention or the roll's content, candidates A–D, and with
  it the HDR headroom — is
  [`nf-calibration/anchor-comparison`](../nf-calibration/anchor-comparison.md)'s, and
  moving `d` is that task's null hypothesis.

## Evidence handed to `anchor-comparison`

Kept verbatim from this task's open questions, since the choice they informed moved
there with them.

- **Should the anchor be at the highlight instead of the mid?** All three outside
  converters anchor the **bright end on content**, at the 96–97th percentile of the
  frame's own density (`docs/reports/three-way-gold200.md`), and that — not any colour
  step — is why their whites read neutral: the channels converge where the shoulder
  and the ceiling bring them together. The measurement also says what to avoid. NLP
  pins each channel's highlight *independently*, which is a per-frame colour decision
  fitted from content, and it is the mechanism behind its worst failure on that roll
  (a\* −21 whole-frame while its top stayed neutral). SilverFast pins one common
  stretch and keeps per-channel balance fixed; it degrades gracefully instead.

  What that suggests, and what this task has to weigh: **the neutral-white effect needs
  the anchor to sit high in density with a shoulder above it — not to be derived from
  content.** A fixed, leader-derived white anchor would give neutral whites when the
  frame actually holds white, and correctly decline to invent one when it does not,
  keeping roll consistency that a per-frame anchor breaks. Against it: a *mid* anchor
  makes contrast and exposure independent (above), which a highlight anchor gives up.

  If a roll-level anchor is considered, two failure modes are already known. A single
  blown frame drags a roll-max anchor up and darkens everything else, so it needs a
  high percentile plus a **minimum gap below the leader's `Dmax`**. And an underexposed
  roll is not a case to normalise away: the film holds less information there, so
  pinning it to white either renders pale or, if contrast is stretched to compensate,
  multiplies noise — measured at up to 4.1× the scan's own floor on a narrow negative.
- **A white anchor is also the HDR crossover, and its placement decides the headroom.**
  The anchor says where `1.0` is; reconstruction must stay **unbounded** through it, so
  everything above flows to the display stage — folded by the SDR shoulder, carried by
  the HDR rendition. A gain map only needs the two to agree *below* that point, so
  diffuse white becomes the natural shared crossover for both branches.

  The trap is placing it at the brightest content instead of at diffuse white: nothing
  can then exceed it, headroom is **zero by construction**, and the result is today's
  inert `GainMapMax` of 1.0× reached by a different route. So the anchor percentile
  *is* the headroom decision. Measured on 2026-09-18-Gold200, stops above the anchor
  up to each frame's p99.99, at gamma 2.0:

  | anchor | median | p90 | max | frames under 0.5 stop |
  |---|---|---|---|---|
  | p95 | 1.23 | 2.01 | 2.51 | 3 of 35 |
  | p97 | 1.03 | 1.78 | 2.22 | 5 of 35 |
  | p99 | 0.67 | 1.33 | 1.77 | 8 of 35 |

  Against the 2.30 stops the containers carry (1000/203), a p97 anchor leaves about a
  stop on a typical frame — modest, but measured, where the shipped default yields
  none. Two consequences: a **fixed or per-roll** anchor suits HDR better than a
  per-frame one, which normalises the headroom away along with the exposure; and the
  frames with under half a stop genuinely have no HDR to deliver, so a near-flat gain
  map there is the honest output and a candidate for a report note.

## How to Verify

- One definition of the constant, with its provenance in a comment beside it.
- No render path gains a read of the datasheet `d_min` unless this task's recorded
  decision says so — and if it does, `film-stock-profiles` Constraint 1 is updated,
  not quietly contradicted.
- A test pins that changing `d` is a **pure gain**: same shape, different brightness.
  (That holds only without a shoulder — the shipped
  `anchor_is_a_pure_gain_only_without_the_shoulder` test states both directions.)
- The four CI gates pass.

## Dependencies

- [The fixed, stock-agnostic decode](fixed-decode.md)
- [Spike: does a diffuse-white anchor earn its place?](anchor-spike.md)
  — **done 2026-09-21.** Costed four white placements instead of choosing one; the
  ranking needs a highlight operator and moved to
  [`nf-calibration/anchor-comparison`](../nf-calibration/anchor-comparison.md). This task
  ships a rule with today's pick and that comparison may move it, which is the
  value-versus-rule split the Decisions section already describes.
