# A home and a name for `linear_range`

## Goal

Decide whether `print.linear_range` survives the migration at all, and if it
does, which stage owns it and what it is called. Retiring it is a legitimate
outcome.

## Design

- It is an **affine levels remap** — a stated `[low, high]` onto `[0, 1]` — not a
  fit-range operator (design-update Part 2, Decisions). The name says "range" and
  the stage it sits beside does range fitting, which is most of the confusion.
- Its current acceptance matrix is scaffolding, not design: display presets take
  it, `legacy` rejects it rather than dropping it silently, `film-master` rejects
  it. Under the new flow there is one chain, so the matrix disappears and the
  question is only which stage it belongs to.
- Candidates, in rough order of how much they cost:
  1. **Retire it.** Exposure (scene correction) and contrast (look) cover the
     everyday uses; a levels remap on scene-referred values also moves mid-grey,
     which is the convention the new anchor exists to pin.
  2. **Keep it in scene correction** as an explicitly-named levels control for
     the cases where a user has a measured range and wants it mapped.
  3. **Keep it in the look**, on the grounds that a deliberate levels move is a
     creative act rather than a correction.
- Whatever survives needs a name that does not collide with fit range, and a
  stated reason it can do something the two knobs above cannot. If no such case
  turns up, that is the answer.

## How to Verify

- If retired: the flag and the recipe key produce a removed-knob error naming the
  replacement, following the `--algorithm` precedent; no silent acceptance.
- If kept: it has one stage, one name, one recipe key, a report field, and a test
  demonstrating the case that exposure plus contrast cannot reach.
- Either way the decision is recorded with its reason — a future reader meeting
  the old key in a recipe should find out what happened to it.

## Dependencies

- [Scene correction as a named stage](stage.md)
