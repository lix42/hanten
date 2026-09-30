# Hanten — nf-docs Progress Log

Execution log for the `nf-docs` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

Fold the new design into the spec, the user guide and CLAUDE.md.

Created on 2026-09-19 with the new-flow migration plan (`docs/nf-migration.md`).
The only change so far is CLAUDE.md's layout (2026-09-24, `claude-md`): it now
holds only cross-cutting rules, and subsystem traps live in module docs.
`design-spec.md` became the design on 2026-09-30 (`design-spec`); `design-update.md`
keeps its evidence (Part 3, the appendices) and a map of where Parts 1–2 went.

## design-spec

**Status:** done
**Updated:** 2026-09-30

- 2026-09-19: created with the new-flow plan. Goal: fold the new design into the spec.
- 2026-09-30: **done.** Scope settled with the user before writing: rewrite every
  section that described the removed chain, not only the three the task named; keep
  retired things as one line each; trim `design-update.md` to its evidence; move the
  open questions into spec §13.
  - **Section numbers kept** (§1–§13, and §12's item numbers): other files cite §8, §9
    and §12 items 1, 3, 6 and 7. A shipped or retired roadmap item shrinks to one line
    and keeps its number.
  - **Where things went.** Principle 2 is now "reconstruction is a fixed decode;
    rendering owns the picture". §5 is the destination set (the axes, the rows, the
    rendering's defaults, the output-path rule, no sidecar). §6 carries design-update
    Part 2: the stage table, the order constraints and the branch contract, what each
    rendering stage is for, the two renderings (with the procedure for keeping
    `direct` current) and the film master. §7 carries Part 1: the model, the decode and
    which of its parameters are really rendering, the decisions, the retired
    reconstructions (§7.3, one line each), calibration toward Status M, and the NC film
    RGB v1 contract (§7.5). §8 keeps the recipe document and its rules, the recipe
    warnings, and a report section rewritten around `chain`. §9 is by stage, the
    knob-level text moved from the old §8 "new chain's recipe", plus one "Retired flags
    and keys" list. The removed chain's §9 print and preset sections are gone.
  - **`design-update.md`** keeps Part 3, Part 4 and the appendices, and opens with a
    table mapping every Part 1–2 heading to its spec section, since closed task files
    and progress logs cite those headings. `src/` pointers, `CLAUDE.md`,
    `scripts/reference-snapshot/README.md`, `docs/datasheets/README.md` and
    `docs/design/README.md` now point at the spec.
  - **Checked against the binary**, not the diff: destination derivation (`--range hdr`,
    `--transfer linear`, `--transfer pq`, `direct` with and without `--range sdr`, the
    planned SDR JPEG), the refusals the spec names (`--film-master` beside a stage knob
    and beside `direct`, the retired flags' messages), a real `convert` and `roll`
    report's shape, and every §8 example invocation on the fixtures.
  - **Found on the way, not fixed here:** the `roll` report no longer echoes the shared
    recipe (only its `identity`); the old spec claimed it did, the new one does not. A
    recipe decode missing a key no longer warns, which the old §7.2 claimed; dropped.
    `docs/using-nc.md` still points at `design-update.md` for the design
    (`nf-docs/using-nc`'s). Closed task files still cite old spec section numbers
    (§7.3 for the sigmoid); they record what was true then.
- 2026-09-30: **two review rounds, then merged `main`.** An nc-reviewer pass and two
  `/code-review` passes, each finding checked against the binary before fixing. Worth
  knowing for the next edit: diffuse white is `1.0` in the **graded** image, ≈0.80 at the
  decode's own output; `direct` warns on exactly two leftovers, and a white balance typed
  beside a `roll` section is dumped and warns on replay; typed `--roll-*` flags are
  refused under `direct` and beside `--film-master`, and do not silence the
  missing-exposure warning; the linear HDR TIFF is clamped at the peak. Merging `main`
  brought `--export-film-rgb` (`nf-verification/film-rgb-export`), now in §6 and §9.

## using-nc

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: bring the guide up to the new flow.

## claude-md

**Status:** in progress — layout done; the architecture rewrite waits on the flip
**Updated:** 2026-09-24

- 2026-09-19: created with the new-flow plan. Goal: update claude.md for the new architecture.
- 2026-09-24: CLAUDE.md restructured (1,428 → ~300 lines) ahead of this task. It
  now holds only cross-cutting rules; each subsystem's traps were checked against,
  or moved into, the `//!` docs of its module, and tool notes into nested
  `tools/review-app/CLAUDE.md`, `scripts/analysis/CLAUDE.md` and skills. The old
  HDR-framing paragraph is gone. What remains for this task: replace the
  current-chain diagram with the new stage sequence once `nf-core/default-flip`
  lands, update the "Where the detail lives" table for retired modules, and
  retire the migration rule.

## reference-sweep

**Status:** done
**Updated:** 2026-09-30

- 2026-09-19: filed after the plan review. Goal: re-point references to retired and superseded tasks.
- 2026-09-30: **done**, after `nf-retire` landed, so this was the task's planned second
  pass. The inactive set is the 16 task files with a *Superseded* or *Retired* header,
  plus the five closed ones (`output/ultrahdr-dependency-externalization`,
  `color/post-reconstruction-color-characterization`, `nf-scene-correction/flare-removal`,
  `nf-look/scene-range-mapping`, `nf-destinations/default-destination`). Grepped for the
  full ids and bare stems across `src/`, the spec, the guide, `CLAUDE.md`, skills, agents
  and scripts; most sites the task file listed (`density.rs`, `stages.rs`, …) had
  gone with the code the retirements deleted. Four were left:
  - `film_base.rs`: the IR premise credited to the retired `film-base/white-holder-support`
    — pointer dropped, fact kept.
  - `using-nc.md`: `--black-point`'s row said a flare/fog subtraction "has not landed";
    `flare-removal` closed it as not needed.
  - `version.rs`: `core/input-semantics` never resolved; the task is
    `io/input-data-semantics`.
  - `version.rs`: "if revisited, `core/conversion-versioning` is its home" for the
    `--auto-d-max` question, which retired with the flag — dropped (no successor).
  Also `types.rs`: a per-stock mid-to-white value "belongs to `algo/film-stock-profiles`"
  (done) — the decode is stock-agnostic now, so it says that instead.
  Left alone on purpose: citations reading as history ("closed", "retired in X"), and
  `design-update.md` Appendix D's `algo/contrast-latitude-spike`, which is evidence.
  Every backticked task id in `src/`, the spec, the guide and `CLAUDE.md` now resolves.
