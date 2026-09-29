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
- **Evidence, 2026-09-29.** A thin 2026-09-28 roll (`20260928-film-1980…2014`,
  machine-local, not yet in `nc-assets`; 1978 leader, 1979 unexposed). Its frames'
  whites were −1.2 to +1.2 scene stops, median about +0.1, so the white bound at the
  floor. That rendered dark (written means about 0.15–0.20). A hand `--exposure 1.4`
  brought the ordinary frames to a normal level with 0.02% clipped at worst, and the user
  accepted it. The darkest five frames stayed dark, which is the intended behaviour.
  +1.4 is what puts the median frame white at the floor.

Open:

- **The statistic.** A central one, so one bright frame cannot set it. Candidates are the
  median of the frame whites `measure-roll` already computes, and a pooled mid-tone
  statistic (e.g. log-average luminance). The roll's white stays a max-like value under
  the cap.
- **The target**, in scene stops: the floor (+1.5), or a value of its own.
- **The order.** The whites are measured at exposure 0 today. With a roll exposure,
  the white and the clamps should be measured after it. Then decide whether the floor,
  cap and clamps keep their meaning, or need a re-review.
- **Bounds.** Most likely about ±2 EV. An over-exposed roll gets a negative value. What a
  bound that binds reports, and what `--strict` does with it.
- **Renderings.** Whether `direct` applies it, as with the other `roll` values.
- **The stated rationale moves.** `algo/fixed.rs` says the fixed anchor "lets film
  speed show through, which is the faithful behaviour". The decode stays true to that,
  but with a measured roll exposure a roll no longer shows its film speed. Update that
  doc, the user guide's §7, and `docs/design/roll-workflow.md` to match.
- **Display black.** A positive exposure raises the base toward mid-grey. At the roll's
  contrast, check how close the bounds get to display black's warning.

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
