# Spike: opt-in bounded scene-range mapping

## Goal

Find out whether measuring a frame's own range and mapping it to the output earns
a place as a per-frame opt-in. This is a spike: "we tried it and it stays off" is
a complete and acceptable outcome.

## Design

- **The idea is what NLP does** (design-update Appendix D), and a frame filled by
  one surface is where it visibly fails. **Do not start from a mechanism:** the
  appendix is explicit that the single-surface collapse is an observation to
  explain, not evidence — a monotone stretch destroys nothing by itself, and which
  further stage loses the detail (end clipping, post-stretch quantization, a
  flattening nonlinearity) is unidentified. Capping the gain is the obvious first
  guard, not a diagnosis. Locating the mechanism is part of this spike, or it
  picks a remedy for a fault it never found.
- **Shape:** measure the frame, cap the gain, and let the result set **exposure**
  (scene correction) and **contrast** (look) — no new operator, just two existing
  knobs resolved from a measurement instead of from a flag. That keeps the render
  path identical whether it fires or not, which is also what makes the comparison
  clean.
- **Never the default, and never silently per roll.** Roll consistency is the
  product's central promise: a per-frame measurement is precisely what breaks it,
  which is why this is opt-in per frame. A *roll-level* solve — one measurement
  across frames, applied to all of them — is a different and possibly better idea,
  and is in scope for the spike to consider.
- **Reuse the statistics that exist** — effective-area statistics shipped in
  `#126`; a second path over the same pixels is a second source of truth.
- **State the success criterion before rendering anything:** it must beat the
  fixed default on the frames it is aimed at without making any other frame worse.
  A spike ending in "sometimes better", with no rule for when, has not concluded.

## Open questions

- What is measured — which percentiles, over what area, in which domain — and
  what the ceiling on the gain is.
- What happens at the ceiling: clamp silently, clamp and report, or decline.
- Whether it also has to be roll-stable when several frames opt in.

## How to Verify

- A review set over one roll with it off, on, and clipped at the ceiling
  (`nctool review generate`), with the decode and every other stage held fixed.
- The outcome is recorded in the progress log either way, including the negative
  one — that record is the deliverable if it stays off.
- If it ships: the report says it fired and by how much, and the fixed default is
  reachable by doing nothing.

## Dependencies

- [The look stage](stage.md)
- [Scene correction as a named stage](../nf-scene-correction/stage.md)
