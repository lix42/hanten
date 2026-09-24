# The look stage

## Goal

The creative stage exists: a named place in the chain with its own recipe
section and report fields, empty by default and doing nothing until the controls
below land in it. It is the stage nc does not have today.

## Design

- **Where it sits: after scene correction, before the SDR/HDR branch**,
  scene-referred and linear. This is a constraint, not a preference — a gain map
  requires the two renditions to agree below diffuse white, so anything shaping
  midtone character must be applied once, above the branch. Only fit range and
  later may differ per branch (design-update Part 2, "The stages").
- **Recipe section.** The `print.*` prefix retires with the legacy path; the look
  picks its own name. Which knobs live under it is settled by the three control
  tasks; this task settles the container and whether it is one object or several
  keys (Part 2 leaves the CDL-versus-separate-knobs spelling open).
- **`film-master` gets no look.** It is reconstruction's output and the artifact a
  reconstruction is measured on. A stated look on `film-master` is therefore a
  **refusal**, not a silent drop — the project's ignored-knob rule.
- **The validate rules need a shape that does not grow per knob.** Today
  `cli::validate_output_preset` enumerates which branch accepts `linear_range`
  and which display tones, one arm at a time, and CLAUDE.md records four shipped
  cases of those rules mis-diagnosing. One rule saying "this destination runs no
  look" beats one rule per look knob.
- **An empty look is reported as empty, not absent**, so a report always says
  whether a look ran. That also keeps the "prose claims must follow the resolved
  config" rule reachable: the stage's report is derived, never asserted.

## How to Verify

- An empty look is a bit-exact identity through the stage.
- `film-master` with a look the user set — one that is neither the default nor
  empty — refuses, with a message naming the look rather than a downstream knob.
- The report names the stage in both the empty and the configured case.
- One recipe drives the same look through every destination that has one.

## Outcome (2026-09-23)

Closed as a decision task: the stage, its empty recipe section, its report entry
and its identity guarantee already existed from `nf-core`. Settled here: one key
per control under `look`; an empty look reports `"identity"`. Deferred, because
nothing can reach them until a look knob and a second destination exist: the
non-empty predicate (added by the first look control), the `film-master` refusal
(verified by the first look control or `nf-destinations/preset-set`, whichever
lands second), and one recipe through every destination (`preset-set`).

## Dependencies

- [Scene correction as a named stage](../nf-scene-correction/stage.md)
