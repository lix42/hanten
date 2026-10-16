# Frame offsets

What does a frame's override mean for each kind of knob, in the desktop app
(`docs/design/desktop-app.md` §2.3)? Produced by `app/frame-offset-spike`, 2026-10-16.

A frame override is an offset on the roll: when the roll's value moves, the frame keeps
its difference. For exposure that is a sum in EV. Contrast and saturation are multipliers
on a slope, so the offset could be a **ratio** (the frame's value is the roll's × r) or a
**step** (the roll's + d).

## The two rules

A frame's slope is its base slope × its multiplier. The base differs per frame, but it
multiplies whatever the rule produces, so it scales how visible a difference is, not which
rule keeps the tweak. The rules separate when the roll moves:

| A frame tweaked at roll 1.0 to 0.7, then the roll moves to | 0.6 | 1.0 | 1.3 | 1.5 | 2.0 |
|---|---|---|---|---|---|
| ratio (× 0.7) | 0.42 | 0.7 | 0.91 | 1.05 | 1.4 |
| step (− 0.3) | 0.3 | 0.7 | 1.0 | 1.2 | 1.7 |
| step's tweak, as a share of the roll's value | −50 % | −30 % | −23 % | −20 % | −15 % |

- **A ratio keeps the tweak's share**: the frame stays 30 % flatter than its siblings
  at every roll value. A step shrinks it as the roll rises and grows it as the roll falls.
- **A step can leave the knob's range.** Contrast and saturation must be positive; a
  frame offset by −0.6 under a roll lowered to 0.5 has none. A ratio never leaves it.
- **A ratio is the knob's own algebra.** `--contrast` and `--saturation` are already
  ratios on a base, and `--white-balance` multiplies the roll's gains. A ratio on a ratio
  is one ratio; on a slider in log scale it is a fixed distance, as an EV offset is.
- **The gap is the tweak times the roll's move**: (1 + a)(1 + b) against 1 + a + b. Two
  moves of 0.1 differ by 0.01; two of 0.3 by 0.09, which is 22 % of the step's value when
  both go down.

## Measured

`2026-09-29-Ektar100`, re-measured whole (`measure-roll`, `pipeline_version` 17,
`370ac04`); report numbers only. Scripts and renders are in `../temp/frame-offset-spike/`.

| Placement | Contrast base slope over 31 frames |
|---|---|
| hybrid (default) | 1.55–3.50, each frame its own placed slope (3.5 is the cap) |
| roll | 1.65 for most; thin frames 2052 and 2053 at 2.03 and 2.05; 2031 and 2037 at 1.55 |
| frame | 1.34–3.50 |

The saturation base is the roll's 1.89, except 1.78 on 2031 and 2037, whose clamped white
gives them their own roll slope. So under the default placement one contrast offset moves
the flattest and steepest frames' slopes by amounts 2.3× apart, under either rule.

## Review

Eight frames of the roll (2026, 2033, 2035, 2037, 2038, 2044, 2046, 2052; contrast base slopes
1.55–3.5), hybrid placement, SDR Display P3. A frame tweaked at roll 1.0, then the roll
moved to 1.5. Each set held five cells: the roll and the tweaked frame before the move,
and after it the roll, the ratio rule and the step rule.

| Knob | Tweak at 1.0 | After the roll's move to 1.5: ratio, step | Which kept the tweak |
|---|---|---|---|
| contrast | 0.7 | 1.05, 1.2 | ratio |
| saturation | 0.6 | 0.9, 1.1 | ratio, weakly ("hard to answer") |

**Decision: a frame's contrast and saturation offsets are ratios.** The review chose the
ratio for contrast. For saturation the review barely separated them, and the reasons above
decide.

## Per kind of knob

| Knob | A frame's offset | Identity |
|---|---|---|
| exposure | a step in EV, added to the roll's | 0 EV |
| contrast, saturation | a ratio on the roll's multiplier | × 1 |
| overall cast (`--white-balance`) | a ratio per channel on the roll's stated gains | × 1, 1, 1 |
| tone cast (`--channel-grade`) | a ratio per channel on the roll's exponents | × 1, 1 |
| tier-3 switches and named steps | a plain override: the frame's own value, which does not follow the roll | — |

- **The colour controls are ratios too.** `--white-balance` already multiplies the
  measured gains, and the grade's exponents, like the slopes, must stay positive.
- **A ratio cannot leave a positive range, but the grade has a second bound.** The spread
  over `[r, 1, b]` must stay under 1, so moving the roll's grade can push an overridden
  frame past it. The core refuses that roll change and names the frames; it never clamps
  them.
- **An offset at its identity is no override.** A plain override equal to the roll's value
  is still one: it pins the frame, so the frame will not follow a later change.
- **Reset** clears the frame's override of that knob; the frame then follows the roll.
- **Copy config** copies the offset (or the plain override), not the frame's resolved value.
- **Lift** moves one frame's value to the roll:
  - the roll takes the source frame's resolved value, and the source's override clears;
  - another frame with an offset of the knob keeps its look, so its offset is restated
    against the new roll value (ratio ÷ the source's ratio, step − the source's step);
    a plain override needs no change;
  - frames with no override follow the new roll value, which is what lifting is for.
  Lift takes one source value: with frames of different values selected, it is
  unavailable.

## The UI

- A frame's control shows the frame's value and marks the roll's beside it, with the
  offset in the knob's own unit: `+0.3 EV`, `× 0.7`.
- Ratio knobs sit on log-scale sliders, so × 2 and × 0.5 are the same distance from the roll
  and a frame's thumb moves with the roll's at a fixed distance, as an EV offset does.

## Not settled

- Dragging a control with several frames of different values selected: does each frame's
  offset change by the same ratio, or do they all take one value?
- The slider ranges and their steps.
