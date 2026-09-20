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
