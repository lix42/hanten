# A parametric operator with a toe

## Goal

Decide whether fit range earns a parametric operator — one with a toe, contrast and
a display-peak parameter — in place of reinhard. Add it only if it wins at matched
lightness.

## Design

Why the question exists:

- **Reinhard compresses upward only.** Measured on the shipped operator at the
  default headroom, the local slope is 1.00 at 0.002 (design-update Part 2, "The
  shadow end") — below mid it is a gain, not a curve.
- **So the shadow end is shaped by a black-point subtraction, which crushes** —
  reaching black by taking light away is exactly what a toe exists to avoid. The
  same section records the measured share of frames driven to code 0.
- **An operator with a toe could hold both mid-grey and diffuse white**, which
  reinhard cannot: it preserves mid by construction and pays about a stop at
  diffuse white. That trade is the thing to test.

What this task supersedes:

- **`docs/tasks/algo/content-aware-sigmoid-toe.md`.** What survives is its evidence
  that a toe is worth wanting at all; what died is its *placement* — a toe in the
  reconstruction curve, which measured as buying nothing and costing black depth. A
  toe belongs where the display range is known. The content-derived half does not
  carry over either; a measured scene range, if it returns, is an `nf-look` opt-in.

Open:

- **What "beats reinhard" means as a test.** Matched lightness is the floor, the
  judgement is a visual review, and a candidate that wins on numbers can lose on
  the picture (the 2026-09-17 offset round).
- **Whether the toe replaces the display-black subtraction or joins it** — i.e.
  whether the black point survives the split at all.
- How many parameters are exposed versus fixed per destination: an operator with
  four knobs is a look surface, and the look stage already owns contrast.

## How to Verify

- A review set at matched lightness across real frames, one decode held fixed, and
  a recorded verdict — including "reinhard stands" as a valid outcome.
- If it ships: the crushed-shadow share measured before and after, and the operator
  named in the report rather than implied by prose.

## Dependencies

- [Fit range as one stage](fit-range.md) — the operator is a setting of that stage
