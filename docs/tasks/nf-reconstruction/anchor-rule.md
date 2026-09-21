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
only in a `#[cfg(test)]` table (`MID_ABOVE_BASE`, `src/algo/film_stock/mod.rs`), and
the datasheet `d_min` in `film_stock/curves.rs` is documented diagnostic-only — no
render path reads it (`film-stock-profiles` Constraint 1). So this task must
**decide**: lift that prohibition, or hand-freeze one constant with its derivation
recorded beside it. Hand-freezing keeps the constraint intact; lifting it makes a
datasheet a runtime input for a value that must not vary per stock. Resolve that
tension rather than finesse it.

## Open questions

- **The value itself.** ≈0.62 is today's pick and is expected to move — by visual
  review now, by the bracketed calibration frames later. Moving it costs a
  `pipeline_version` bump, which is planned rather than a regression.
- **Does `d` stay user-reachable?** Every value like this is a flag and a recipe key
  today; the design deliberately does not settle whether it should remain one.
- **What the other three placements become** — removed, or refused. That answer is an
  input to [the audit](../nf-core/knob-availability-audit.md).
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
  — measures the highlight-anchor question below instead of arguing it
