# Calibrate a roll without named reference frames

**Design:** [the roll workflow](../../design/roll-workflow.md) — the single source for this task's CLI shape; where they disagree, the doc wins.

> **Renamed 2026-09-28** from `core/base-acquisition-planner`. The explicit half of
> that plan — measure the base from a frame you name and write it as a recipe — moved to
> [`core/measure-base`](measure-base.md) (which also makes `measure-roll` measure the
> base), and the one-command roll to [`core/roll-measure-mode`](roll-measure-mode.md). What
> stays here is the **automatic** half.
> The old file's `Dmax` rung and its `hanten calibrate` command are gone: the roll
> reference density retired (`nf-retire/dmax-machinery`), and the command's role went to
> `measure-base` and `measure-roll --unexposed` (both `core/measure-base`).

## Goal

When the user does not name the unexposed frame or the leader, find them in the roll,
resolve the roll's calibration from whatever is there, record which source won and how
confident it is, and drop **loudly** to single-frame conversion when nothing is
trustworthy. This is an opt-in mode of `measure-roll` (and so of `roll`'s measure mode), never
its default (2026-09-28: finding the frames is measurement).

## What is known

- **Plan → frozen recipe → deterministic apply.** Every heuristic lives here and
  resolves to explicit values; conversion is replay. Provenance (which rung, which
  frame or region, the confidence, the cross-frame spread) goes in the report.
- **The base cascade, most reliable first:** a dedicated unexposed frame → a named
  region on a picture frame → the automatic measurement → cross-frame agreement over
  the roll → drop to single. Content-based estimation (`film-base/content-fallback`)
  is **never** a rung: it is scene-dependent, and is used only on explicit opt-in.
- **The automatic rung is changing.** `film-base/holder-masked-measurement` rebuilds
  the base measurement on area × method and retires the rebate-band search, so
  "try auto" means "measure the effective area", not "detect a rebate".
- **Cross-frame agreement** is the roll's corroborator: the same base from ≥ 2 frames
  beats one frame's edge. One frame alone is uncorroborated — low confidence.
- **Reference frames are detectable but never assumed:** an unexposed frame is
  whole-frame uniform base; a leader is whole-frame dense (dark RGB, bright IR). A
  blank sky or a lightbox shot can fool either, so a detection is confirmable and
  overridable.
- **The leader now guards `measure-roll`** (saturation check, white-balance guard)
  rather than measuring a reference density; detecting it serves that.

## Open questions

- How confident must a detected frame be before it is used without confirmation?
- Does a detected leader or unexposed frame get excluded from the picture frames
  automatically, given `measure-roll` refuses the leader among its inputs?
- What does "drop to single" produce — per-frame auto bases, or a refusal?

## How to Verify

- A roll with its unexposed frame and leader among the inputs, neither named: both
  are detected, the base and the roll's white balance and white freeze, and the
  report records the provenance.
- Two frames whose bases agree corroborate each other, with the spread recorded; a
  single candidate is reported as uncorroborated.
- A roll with no usable reference drops to single conversion loudly; content
  estimation runs only when explicitly opted in.

## Dependencies

- [Roll conversion](roll-conversion.md)
- [The `calibration` recipe section](calibration-recipe-section.md)
- [Robust auto film-base detection](../film-base/auto-base-redesign.md)
- [IR-assisted film-holder detection](../film-base/ir-holder-detection.md)
- [Rebuild Dmin and Dmax measurement on area x method](../film-base/holder-masked-measurement.md)
  — replaces the automatic rung
- [`measure-base`](measure-base.md) — the explicit measurement this automates, and
  `measure-roll --unexposed`, the command this is a mode of

Related, not a dependency: [content-based film-base
fallback](../film-base/content-fallback.md) is opt-in only and never enters the
automatic cascade.
