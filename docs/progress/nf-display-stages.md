# Hanten — nf-display-stages Progress Log

Execution log for the `nf-display-stages` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

Fit range and fit gamut as real stages, shared by both display branches, plus the operator question the shadow end raises.

Nothing has landed yet: the epic was created on 2026-09-19 as part of the new-flow
migration plan (`docs/nf-migration.md`).

**One measurement here blocks an `nf-look` task.** `gamut-map-share` (filed 2026-09-22)
asks how much highlight chroma convergence the existing gamut map already produces.
Nothing turns the map off by flag, so every desaturation measurement taken so far reads
shoulder-plus-gamut-map jointly — which is why `nf-look/path-to-white` depends on it, and
why it runs against today's binary rather than waiting for `fit-gamut`.

## fit-range

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: fit range as one stage.

## fit-gamut

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: one gamut-mapping implementation.

## parametric-operator

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: a parametric operator with a toe.

## branch-contract

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: the sdr/hdr branch contract.

## gamut-map-share

**Status:** not started
**Updated:** 2026-09-22

- 2026-09-22: filed, split out of `nf-look/path-to-white`. Goal: how much of the
  highlight chroma convergence nc already produces is the **gamut map** rather than a
  tone or reconstruction curve. Two things rest on it: `path-to-white` must not double up
  with the map, and the map is one of only two surviving candidates for the knee'd
  sigmoid's clean whites after design-update Appendix F eliminated the other three
  (`sdr.rs:249-266`, radial near luminance 1.0 with a ceiling that follows the rendered
  luminance — which is how the display tone reaches it indirectly although no nc tone can
  converge channels itself). Nothing turns the map off by flag, which is exactly why
  `nf-look/desaturation-spike` could not separate them; `--display-tone shoulder` failed
  as a separator because it drives top-end chroma to 0.0 with 17-29 degrees of rotation,
  i.e. it flattens rather than shapes. Runs against today's binary. Two routes: a
  throwaway patch disabling the map (measures the share) or counting top-end samples out
  of gamut before mapping (bounds it, no patch).
