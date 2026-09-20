# The scene-referred half of the black point

## Goal

Give flare/fog removal a place of its own in scene correction, so that reaching
black at the display end stops being done by subtracting from scene-referred
values.

## Design

- Today one linear subtraction (`print.black_point`) does two jobs at once. It is
  the wrong instrument for the second: measured 2026-09-02
  (`docs/progress/algo.md`), 0.019 crushed 0.69–8.66 % of every frame to code 0,
  and ≈0.005 is the largest fixed value that crushes nothing — which is too small
  to place black. Display black and the approach to it belong to fit range
  (`nf-display-stages`); only the scene-referred term belongs here.
- What this stage removes is **veiling glare and base fog**: an additive term the
  lens and the scanner contribute, present before any display decision. Removing
  it is a correction toward what the scene was, which is this stage's job
  description.
- **Do not re-create one knob spanning both jobs.** The split is the point; a
  single number that a user nudges until blacks look right is exactly today's
  behaviour under a new name.

## Open questions

- Scalar or per-channel? Fog is per-layer on film, but the term acting here is
  after the 3×3.
- Stated, or measured from the frame? No flare estimate exists today — the
  measurement in the repo is a *crush count*, which grades a candidate rather than
  producing one.
- Does it default to zero? Probably: a non-zero default subtraction is what
  produced the crushing above. A non-zero default has to be argued from a
  measurement, which is `nf-calibration`'s business.
- Interaction with the decode's `offset`, which is also an additive density-domain
  term — they are not the same quantity, and the report should not let them read
  as one.

## How to Verify

- Default renders crush nothing on the frames the 2026-09-02 set measured;
  `pipeline::shadow_metrics`' `crushed%` is the metric that can see it.
- The resolved term is reported, separately from whatever fit range does at the
  bottom end.
- A stated value reproduces on a synthetic frame with a known additive pedestal.

## Dependencies

- [Scene correction as a named stage](stage.md)
