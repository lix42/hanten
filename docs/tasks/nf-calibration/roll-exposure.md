# A measured roll exposure

## Goal

`measure-roll` measures a roll's exposure and writes it as `roll.exposure`, beside
`roll.white_balance` and `roll.white_stops`, so an under- or over-exposed roll renders
at a normal level without a hand-picked `--exposure`. It is one value for the whole
roll: a frame darker than the roll stays dark, so the render shows what is on the film
rather than hiding it.

## Design

What is known:

- **Why the white rule cannot do it.** The rule solves only contrast, with mid-grey
  pinned. When a roll's frame whites sit near or under mid-grey, no contrast brings them
  up: a steeper one pushes a sub-mid white further down. The floor keeps the contrast
  sane on such a roll, but it cannot lift the roll.
- **Make it an exposure, not a moved anchor.** A density shift of the decode's anchor
  goes through the per-channel `density.scale` and tints the roll. A single gain in
  linear ACEScg (what `scene_correction.exposure` already is) is neutral. `d` stays
  0.62 and the decode stays stock-agnostic (`algo/fixed.rs`).
- **Pool it over the roll; never per frame.** A per-frame value reads a night scene as
  under-exposure, the same failure that retired per-frame auto white balance
  (`nf-scene-correction/roll-white-balance`).
- **Follow the `roll` section's pattern** (`roll-section`): a measured value lives in
  `roll`, apart from the style knobs. It needs a flag as well as the key. The
  stated `--exposure` then adjusts it rather than replacing it, the way `--white-balance`
  multiplies the roll's gains, and the report says both.
- **Evidence, 2026-09-29.** A thin 2026-09-28 roll (`nc-assets` roll
  `2026-09-28-Portra400-dark`; scanned as `20260928-film-1978…2014`, 1978 the leader, 1979
  the unexposed frame). Its frames'
  whites were −1.2 to +1.2 scene stops, median about +0.1, so the white bound at the
  floor. That rendered dark (written means about 0.15–0.20). A hand `--exposure 1.4`
  brought the ordinary frames to a normal level with 0.02% clipped at worst, and the user
  accepted it. The darkest five frames stayed dark, which is the intended behaviour.
  +1.4 is what puts the median frame white at the floor.

Settled (details in `docs/progress/nf-calibration.md`, `## roll-exposure`):

- **The statistic:** the median over frames of each frame's log-average ACEScg luma.
- **The target:** −0.6 scene stops from mid-grey.
- **The order:** the white and the clamps stay measured at exposure 0.
- **Bounds:** ±2 EV; a bound that binds warns, so `--strict` refuses it.
- **Renderings:** `direct` leaves it out, as it does the other `roll` values.
- **The stated rationale moves:** done — `algo/fixed.rs`, the user guide's §7 and
  `docs/design/roll-workflow.md` updated.
- **Display black:** on the three dark rolls the shallowest rendered film base sat 2.61
  stops under mid-grey, clear of the 2.0-stop warning.

## How to Verify

- A review set across the nine rolls plus the 2026-09-28 roll, comparing the measured
  exposure against a hand-chosen one. On that roll it should land near +1.4. Well-exposed
  rolls should come out near 0, and their renders should barely move.
- `measure-roll --out` writes `roll.exposure`. `roll` and `convert` apply it, the
  report states it, and `--exposure` adds to it. A pre-existing recipe with no
  `roll.exposure` renders unchanged.
- The goldens and the drift gate still hold for a render without a roll measurement.

## Dependencies

- [`measure-roll` places the roll's white](roll-white-rule.md) — the whites the
  statistic starts from, and the rule whose order changes
- [The roll's measurements as their own recipe section](roll-section.md) — the section
  the value joins
