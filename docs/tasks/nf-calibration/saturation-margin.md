# The saturation warning's margin, and frames near saturation

## Goal

Settle when `measure-roll` warns that a frame is near film saturation. The warning keys
on the frame's distance from its leader today, with a placeholder margin (0.5 stop).

## Design

What is known (`docs/progress/nf-calibration.md`: `anchor-comparison` 2026-09-25, and
this task's measurement on 2026-09-29):

- **The user's verdicts**, as white in scene stops above mid-grey / distance under the
  leader: 1121 +4.51 / 0.22 (overexposed), 1151 +3.15 / 0.38 (ambiguous), 1868
  +3.26 / 1.85 (a bright scene), 1816 +2.10 / 0.29 (not judged overexposed). 1635 and
  1815 warn today and are unjudged. That is **one** overexposed frame.
- **The leader is not a stock constant.** Across the ten rolls it sits +2.39 to +5.65
  stops above mid-grey, and within one stock it varies by up to 1.8 stops (Gold200 +3.53
  and +2.39). How far a leader sits above content depends on the roll, so a margin per
  stock has no support, and `measure-roll` knows no stock anyway.
- **Several leaders sit below the datasheets' shoulders** (red shoulder onset, roughly:
  Gold200 +3.9, Portra160 +4.6, Ektar100 +5.3, Portra400 none within the sheet; scanner
  density is uncalibrated against the sheets). A leader then marks how much light it
  got, not where the film saturates. 1816 warns only because 09-18's leader is the
  lowest of the ten.
- **A frame's absolute white separates the verdicts; its leader distance does not.**
  By distance, 1816 (not overexposed) falls between 1121 and 1151. By white, 1121 stands
  more than a stop above every other frame. An absolute level also works without a
  leader. It rests on one frame, so it is a candidate, not a finding.
- **Treatment: the warning only.** The clamp to the cap already gave 1121 the safer
  highlight in review. A treatment of its own becomes a follow-up only if the bracket
  shows the clamp is not enough.

Open:

- Which reference the warning keys on — the leader distance, an absolute white level,
  or both — and its value.
- The fallback without a leader, which an absolute level would answer.

## How to Verify

On the calibration shoot's over-exposed frames (+3/+4 on at least two stocks, one of them
Gold200), plus the rolls' judged frames, the chosen reference and value separate the
frames the user calls saturated from the rest.

## Dependencies

- [`measure-roll` places the roll's white](roll-white-rule.md) — the warning this tunes
- [Capture the calibration frames](../analysis/calibration-frame-capture.md) — the
  over-exposed bracket frames the value is set from
