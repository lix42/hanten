---
title: Value-domain terminology & Dmin/Dmax clarity
status: done
deps: [core/pipeline-orchestration]
tags: [post-mvp, cleanup]
summary: one terminology doc (value spaces + per-stage quantities, public vs internal) and an agent skill; "film base" leads over `Dmin` in user-facing text
---
# Value-domain terminology

## Goal

Make Hanten's terminology easy to understand and use consistently, for people and
agents, without changing the data flow or any public identifier.

## Why

Two kinds of confusion keep recurring:

- **Value spaces.** design-spec §4's "Terminology & value domains" is correct but
  buried under "Input formats". `Dmin` is named for a density but is a per-channel
  *transmission*.
- **The multiplier family.** `slope`, `gain`, `contrast`, `scale`, `linearization`,
  `gamma`, `exposure`, `offset`, `tilt`, `headroom` … Each stage tends to own one word
  (`scale` in the decode, `contrast` in the look), but some words, `slope` above all,
  are used across stages for different quantities, and some quantities go by
  several words.

## Scope (rescoped 2026-10-14)

1. **Inventory** every meaning of the multiplier family and the value-space terms:
   stage, value space, **public** (flag, recipe key, report field, message) or
   **internal** (identifier, comment), and every word in use for it. The user reviews
   the proposed canonical term set before anything is written.
2. **`docs/terminology.md`**: value spaces (from design-spec §4, which keeps a short
   summary and a link), quantities by stage, each marked public or internal, and the
   known collisions. `algo::fixed`'s "Five quantities" table moves there too.
3. **"Film base" leads over `Dmin`** in user-facing text (help, messages, the guide);
   `Dmin` stays a defined alias.
4. **Prose follows the doc**: guide, spec, doc comments where a word misleads.
5. **A thin agent skill** pointing at the doc, plus the `CLAUDE.md` and agent-primer
   pointers.

Out of scope:

- **Renaming code identifiers.** That is a follow-up task, filed from the inventory.
- **Changing public flags, keys and fields.** They are the scripting contract; a bad
  one gets its own task with a migration.

Already done elsewhere:

- `dmax` was retired by `nf-retire/dmax-machinery`, so the old `--d-max` help-text
  item is moot.
- `dmax`'s move into `calibration` was `core/calibration-recipe-section`.

## Constraints

- Preserve the data flow. No pixel, recipe or report changes.
- Terms stay stock-general.

## Dependencies

- [Pipeline orchestration](pipeline-orchestration.md)
