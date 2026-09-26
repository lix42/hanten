# A parametric operator with a toe

## Goal

Decide whether fit range earns a parametric operator — one with a toe, contrast and
a display-peak parameter — in place of reinhard. Add it only if it wins at matched
lightness.

**And place black, whichever operator wins.** The new chain has no black point at all,
and [`anchor-comparison`](../nf-calibration/anchor-comparison.md)'s white rule was chosen
with one in the chain; without it every white placement looks pale. So "reinhard stands"
no longer ends this task. It leaves black to be placed another way.

## Design

Why the question exists:

- **Reinhard compresses upward only.** Measured on the shipped operator at the
  default headroom, the local slope is 1.00 at 0.002 and 0.94 near 0.05
  (design-update Part 2, "The shadow end") — below mid it is nearly a gain.
- **So the shadow end is shaped by a black-point subtraction, which crushes** —
  reaching black by taking light away is exactly what a toe exists to avoid. The
  same section records the measured share of frames driven to code 0.
- **An operator with a toe could hold both mid-grey and diffuse white**, which
  reinhard cannot: it preserves mid by construction and pays about a stop at
  diffuse white. That trade is the thing to test.

Black, from `anchor-comparison` (2026-09-25, `docs/progress/nf-calibration.md`):

- **The new chain places no black.** `--black-point` is refused under `--new-flow`, and
  reinhard is nearly a gain below mid, so the only thing darkening shadows is the look's
  contrast. At the chosen white rule the darkest 1% of pixels sat at L\* 12–26.
- **The reference is already measured: where the film base renders.** Every roll's
  darkest pixels bottom out at the base (red p0.5 at −3.6 to −3.8 scene stops below
  mid-grey, which is 3.7 stops above base), because below the film's threshold nothing is
  recorded. The straight-line decode renders the base at L\* 12–15 at the rule's contrast.
  Moving that level to near black needs no image statistic, and is the same for every
  frame of a roll.
- **What the review showed.** A probe moved the base to L\* ≈ 2 with a linear-light offset
  on the finished JPEG (not a pipeline stage), one value per roll and contrast. With it,
  every white rule improved, and the user's wish for more contrast on two rolls was met
  by the black alone. The probe was a *subtraction* — exactly what this task says crushes
  — so it sets the bar to match, not the mechanism.
- **What stays scene-side.** Scanner veil and base fog are an additive scene term,
  [`flare-removal`](../nf-scene-correction/flare-removal.md)'s. Where the base lands on the
  display is this task's.

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
- **The black target and its mechanism.** L\* ≈ 2 was the probe's untuned value. Whether
  black is a toe reaching the base's level, an offset, or both, and how it keeps the
  SDR/HDR branches in agreement below diffuse white (`branch-contract`).

- **Where a candidate plugs in** (2026-09-23): fit range is `r(Y)·(1 + (P − 1)·s(Y))`,
  so a candidate replaces the base `r`. Keeping the peak lift on top keeps the exact
  below-white agreement between branches; the report's `fit_range.operator` names
  whichever runs.

## How to Verify

- A review set at matched lightness across real frames, one decode held fixed, and
  a recorded verdict — including "reinhard stands" as a valid outcome.
- If it ships: the crushed-shadow share measured before and after, and the operator
  named in the report rather than implied by prose.
- Black: renders on `anchor-comparison`'s frames (`../temp/anchor-cap/`, the rule's arms)
  match or beat the black probe's, with the share of samples driven to code 0 counted —
  the old `--black-point` crushed 0.69–8.66% of every frame.

## Dependencies

- [Fit range as one stage](fit-range.md) — the operator is a setting of that stage
