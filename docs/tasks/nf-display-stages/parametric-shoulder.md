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

**Decided 2026-09-27 (user): reinhard stands.** No operator, knob or report change.
The numbers and the round are in `docs/progress/nf-display-stages.md`.

- **With mid-grey, white and the peak all pinned there is almost nothing to choose.**
  A curve that joins reinhard smoothly at mid-grey, renders diffuse white where
  reinhard does and reaches the SDR peak at the same headroom stays within 0.05 stop of
  it below white and 0.15 stop above. That is under what the white rule's review could
  tell apart.
- **Pinning white alone duplicates existing knobs.** Moving mid-grey with white held is
  exposure plus contrast, and it would move the pivots display black and the white rule
  key on.
- **Pinning mid-grey alone was the one real option**: white rendered brighter with the
  shadows untouched, paid for by the speculars. It was reviewed at +0.15 and +0.30 stop
  and reinhard was preferred.
- So the open questions (what "beats reinhard" means, how many parameters) are moot.
  Where white renders is already a knob, `look.contrast` (via the white rule).
  `--display-tone-headroom` barely moves white; it sets how hard the stops above white
  are compressed.

## How to Verify

- A review set at matched lightness across real frames, the decode and display black
  held fixed, and a recorded verdict.
- If it ships: the operator named in the report, the branch contract re-checked, and
  the white rule's cap and floor re-checked under it.

**How it was met:** at matched lightness in the strict sense (mid-grey, white and the
peak all pinned), the candidates were shown numerically to stay within 0.15 stop of
reinhard, below what the white rule's review could tell apart, so that set was not
rendered. The round rendered instead the one alternative with room: mid-grey and the
shadows pinned (identical rendered L\* p10–p50 across arms), white +0.15 and +0.30
stop. Verdict recorded; nothing shipped, so the second bullet does not apply.

## Dependencies

- [Display black](parametric-operator.md) — judged with black in the chain
- [The roll's white rule](../nf-calibration/roll-white-rule.md) — the white a new
  shoulder would move
