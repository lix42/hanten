# The saturation warning's margin, and frames near saturation

## Goal

Settle how close to its leader a frame's white may get before `measure-roll` warns that
the frame is near film saturation, and decide whether such a frame needs a treatment of
its own.

## Design

What is known (`docs/progress/nf-calibration.md`, `anchor-comparison`, 2026-09-25):

- **The leader tells what the cap cannot.** A frame above the roll's cap is either a
  bright scene well inside the film's latitude or a frame running into the film's
  shoulder, and only its distance from the leader separates the two. Distances in
  scene stops, with the user's verdicts: 1121 0.13 (overexposed), 1151 0.39
  (ambiguous), 1816 0.27 (not judged overexposed), 1868 1.81 (a bright scene).
- **One margin may not fit every stock.** The leader sits ~1.5 stops above the content on
  both Gold200 rolls and ~3.3–3.8 on the Portra400 rolls, so a margin that catches
  Portra's overexposed frames flags ordinary Gold200 ones.
- **Near the leader the film compresses highlights**, and the straight-line decode
  renders that compression flat and bright; clamping such a frame to the cap was the
  review's preference (1121: "much safer at highlight").
- **What `measure-roll` does today** (`roll-white-rule`, 2026-09-27): a frame's white is
  the p97 of its pixels' brightest film-RGB channel over its effective area, measured
  before the leader guard, and its distance is taken to the leader's brightest-channel
  median. At the placeholder 0.5 stop it warns on 1121 (0.22), 1151 (0.38), 1635 (0.20),
  1815 (0.28) and 1816 (0.29). 1816 was not judged overexposed; 1635 (bright content at
  the frame edge, a hand-cropped scan) and 1815 are unjudged. With no leader nothing is
  checked.

Open:

- A margin per stock or one margin, and in what unit.
- Whether a saturated frame gets a different default — a lower contrast, a lower
  exposure, a gentler roll-off — or only the warning.
- The fallback when there is no leader.
- The evidence: the current sample holds no deliberately over- or underexposed frames.
  Collect some before fixing a value.

## How to Verify

A review on frames at known leader distances across more than one stock, recording which
the user calls saturated; the chosen margin separates them.

## Dependencies

- [`measure-roll` places the roll's white](roll-white-rule.md) — the warning this tunes
