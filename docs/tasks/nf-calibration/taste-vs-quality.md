# Separate taste from quality in the automatic adjustments

## Goal

Classify every adjustment Hanten applies on its own as **quality** (it corrects the scan
toward what the film recorded: the roll's white balance, its exposure, the white rule's
placement) or **taste** (a preference: `frame-level-trim`'s small lift, `thin-frame-lift`'s
thin lift). Every taste adjustment gets one documented off switch, and the classification
records what a future GUI needs to preview it and turn it off.

Filed by the user on 2026-10-01: `thin-frame-lift`'s round 2 could not be called, because
the lift is brighter and whether that is better is opinion, not fact.

## Design

Known:

- Today both lifts are turned off together, at render by `--frame-lift off` /
  `roll.frame_lift` and at measurement by `measure-roll --no-frame-lift`;
  `--no-thin-lift` keeps the small lift and drops only the thin one. No render-time switch
  separates the two.
- Both lifts are on by default. The flat-frame guard (`roll_white::FLAT_SPREAD_STOPS`)
  keeps both off a single-surface frame.

Open:

- **Where the line falls** for each adjustment, the white rule's cap and floor included.
- **Whether taste adjustments need one switch or one each.** At render, each lift could be
  turned off alone, or there could be a single "taste" switch.
- **What a GUI needs**: a preview with and without each taste adjustment, from a recipe
  that keeps the measured values while they are off (as `roll.frame_lift` already does).
- **How the report and recipe mark a taste value**, so it reads as a choice.
- **Carried over from `thin-frame-lift`** (closed 2026-10-01 without it): a confirmation
  round on a roll not used to choose its thresholds, and a noise measurement at its
  slopes. Its round 2 could not be called on taste, which is what this task frames.

## How to Verify

- Every automatic adjustment is classified in one place (the design spec), with a reason.
- Every taste adjustment can be turned off at render without re-measuring, by flag and by
  recipe key, and the guide documents each switch.

## Dependencies

- [A bounded lift for a thin frame](thin-frame-lift.md) — the thin lift is a taste
  adjustment, and its switch is part of what is classified
- [A per-frame level trim](frame-level-trim.md) — the small lift, the other taste
  adjustment
