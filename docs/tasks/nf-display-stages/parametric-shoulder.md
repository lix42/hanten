# Does a parametric shoulder beat reinhard?

## Goal

Decide whether fit range earns a parametric operator in place of reinhard's upper
half — one that could hold both mid-grey and diffuse white. Add it only if it wins at
matched lightness; **"reinhard stands" is a complete outcome.**

Split from [`parametric-operator`](parametric-operator.md) on 2026-09-26 (user), which
shipped display black on its own.

## Design

What is known:

- **Reinhard preserves mid-grey by construction and pays about a stop at diffuse
  white.** An operator that held both is the trade to test.
- **The shadow end is no longer this question's.** Display black shapes everything
  below mid-grey (`fit_range`'s module docs), so a candidate need only replace
  reinhard's behaviour from mid-grey up; a toe inside the operator would double up
  with it.
- **Where a candidate plugs in** (2026-09-23): fit range is `r(Y)·(1 + (P − 1)·s(Y))`,
  so a candidate replaces the base `r`. Keeping the peak lift on top keeps the exact
  below-white agreement between branches; the report's `fit_range.operator` names
  whichever runs.
- **It re-opens the white rule.** `nf-calibration/anchor-comparison` chose the roll's
  white under reinhard, and `roll-white-rule` implements it; a shoulder that holds
  diffuse white moves where that white renders, so the rule is re-checked with any
  candidate.

Open:

- **What "beats reinhard" means as a test.** Matched lightness is the floor, the
  judgement is a visual review, and a candidate that wins on numbers can lose on the
  picture (the 2026-09-17 offset round).
- How many parameters are exposed versus fixed per destination: an operator with four
  knobs is a look surface, and the look stage already owns contrast.

## How to Verify

- A review set at matched lightness across real frames, the decode and display black
  held fixed, and a recorded verdict.
- If it ships: the operator named in the report, the branch contract re-checked, and
  the white rule's cap and floor re-checked under it.

## Dependencies

- [Display black](parametric-operator.md) — judged with black in the chain
- [The roll's white rule](../nf-calibration/roll-white-rule.md) — the white a new
  shoulder would move
