# Display black (was: a parametric operator with a toe)

## Goal

**Place black on the new chain.** The new chain had no black point at all, and
[`anchor-comparison`](../nf-calibration/anchor-comparison.md)'s white rule was chosen
with one in the chain; without it every white placement looks pale.

The task was filed to decide whether fit range earns a parametric operator — one with
a toe, contrast and a display-peak parameter — in place of reinhard. On 2026-09-26
(user) that question moved to its own task,
[`parametric-shoulder`](parametric-shoulder.md), so black could ship first: two tasks
wait on black only, and a new shoulder would re-open the white rule chosen under
reinhard. The id keeps its old name.

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
  contrast. In the cap round (contrast ≈ 1.8) the darkest 1% of pixels sat at L\* 12–26.
- **The reference is already measured: where the film base renders.** Every roll's
  darkest pixels bottom out at the base (red p0.5 at −3.6 to −3.8 scene stops below
  mid-grey, which is 3.7 stops above base), because below the film's threshold nothing is
  recorded. Where the straight-line decode renders the base **depends on the contrast**:
  L\* 12–15 at ≈ 1.8 (the cap round), and under the chosen rule L\* 2.6–3.2 on floored
  rolls (≈ 2.95) but 7.2–9.2 on capped ones (≈ 2.25), measured in round 4. So the lift a
  black point must add varies by roll and is near zero at the floor: a fixed offset is the
  wrong shape.
  Moving that level to near black needs no image statistic, but it is **a function of
  each frame's resolved contrast**, not one value per roll. A frame clamped to the cap
  (`roll-white-rule`) renders at 2.23 while its roll may be at 2.97, so its base sits
  higher. Either the mapping takes the frame's contrast, or a toe that handles both levels
  is shown to.
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

Settled by two review rounds (2026-09-26, `docs/progress/nf-display-stages.md`):

- **Mechanism: a shift in stops, on luminance**, whole at the film base and below,
  fading to nothing at mid-grey. It beat a rational toe on colour and shadow contrast,
  and the probe's per-channel subtraction on colour (the probe tints dark
  near-neutrals). Mid-grey and white are untouched, so the white rule stands as chosen
  and the branches still agree below white.
- **Reference: the decoded film base, graded with the frame**, so a frame clamped to
  a different contrast from its roll gets its own shift. No image statistic.
- **Target: a knob**, `--display-black` / `fit_range.display_black`, in stops below
  mid-grey on the display, default 6 (≈ L\* 2.5). The preferred depth followed the
  picture, not the roll. A base already deeper is left alone; `off` disables it.
- **The display-black subtraction does not survive**: the new chain has no
  `--black-point`, and the flare half stays `flare-removal`'s.

The operator questions (what "beats reinhard" means, how many parameters, where a
candidate plugs in) moved to [`parametric-shoulder`](parametric-shoulder.md).

## How to Verify

- Black: renders on `anchor-comparison`'s frames (`../temp/anchor-cap/`, the rule's arms)
  match or beat the black probe's, with the share of samples driven to code 0 counted —
  the old `--black-point` crushed 0.69–8.66% of every frame.
- The branch contract holds with black on (`branch_probe`, 92 frames), and the shift
  is named in the report rather than implied by prose.

## Dependencies

- [Fit range as one stage](fit-range.md) — the operator is a setting of that stage
