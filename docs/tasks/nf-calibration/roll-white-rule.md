# `measure-roll` places the roll's white

## Goal

Implement the white placement [`anchor-comparison`](anchor-comparison.md) chose by review:
`hanten measure-roll` measures the roll's white, solves the look's contrast from it, and
warns on frames near film saturation, and the recipe carries the result as it carries
the roll's white balance.

## Design

What is known — decided by review on nine rolls (the rounds, the numbers and the user's
verdicts are in `docs/progress/nf-calibration.md`, 2026-09-25). The values are
**provisional**: the sample holds no deliberately bad frames.

- **The rule.** Each frame's white is its own high percentile, which keeps the specular
  headroom above white. The roll's white is the brightest frame white **at or under a
  cap** (+2.0 scene stops above mid-grey), **raised to at least a floor** (+1.5). Mid-grey
  stays pinned at the decode's `d`, so the white sets `look.contrast`; over the nine rolls
  the whole contrast lands in 2.25–2.97 per roll, and 2.23 on a frame clamped to the cap.
- **A frame above the cap is clamped to it**, not merely skipped: it renders at the
  contrast the cap gives, not the roll's. It was the review's choice on frames near
  saturation (V3 in the progress log). So the recipe needs a per-frame contrast for those
  frames, not only a roll value.
- **A clamped frame is disclosed, not warned about** (user, 2026-09-25). It renders at a
  different contrast from its roll, so the report states it: which frame, the cap, its
  contrast against the roll's. That is an informational field, since an ordinary bright
  scene (1632, 1902) is clamped too and a warning on it would be noise. The warning below
  stays reserved for film saturation.
- **The warning keys on the leader, not on the cap.** The cap binds on every roll
  measured, so a cap warning would fire on every roll. Warn when a frame's white is
  within a margin of its leader — "near film saturation". Its margin is
  [`saturation-margin`](saturation-margin.md)'s; 0.5 stop is the placeholder. With no
  leader, a fixed fallback.
- **The rule needs a black point.** Every round was judged with the film base moved to
  near black; without it, every placement looked pale and the review would have tuned
  the white to make up for it. Black is
  [`parametric-operator`](../nf-display-stages/parametric-operator.md)'s.
- **Scene stops, not rendered stops**, for every level above: red density through the
  decode's fixed linearization (1 stop ≈ 0.167 density). Rendered stops depend on the
  contrast being solved.

Open:

- **The measurement domain.** The review measured each frame's white as red density p97
  over a 12% inset. `measure-roll` pools ACEScg pixels under a leader guard, and
  `anchor-comparison` asked that the white share that measurement, not add a second one.
  Moving it means re-checking the cap and floor in the new domain.
- **The leader guard hides the frames the warning is for.** `pipeline::roll_white` drops
  pixels within 0.1 density of the leader before taking its percentile, and those are the
  near-saturation highlights. A white measured after the guard lands lower on exactly those
  frames, their distance to the leader grows, and the warning never fires (1121 sits 0.13
  stop, about 0.02 density, under its leader). So measure the saturation distance before
  the guard, even if the roll's white is taken after it.
- Code constants or recipe values — the cap, the floor, the margin.
- How a per-frame clamp reaches a `roll` recipe, and what the report says when a limit
  binds (which one, by how much).
- The user guide (`docs/using-nc.md`), since this changes what `measure-roll` reports and
  what a recipe carries.

## How to Verify

- On the rolls `anchor-comparison` measured, `measure-roll` reproduces its table of roll
  whites and contrasts (2026-09-25) within the tolerance the domain change introduces.
- A frame near its leader warns, including one the leader guard would have excluded; a
  frame merely above the cap does not warn, and the report names it with both contrasts.
- A roll of one dark frame lands on the floor, not on a contrast the floor excludes.

## Dependencies

- [Choose the white placement by rendering](anchor-comparison.md) — the rule and its values
- [A parametric operator with a toe](../nf-display-stages/parametric-operator.md) — places
  black, which the rule was judged with
