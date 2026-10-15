---
title: The roll white as a tilt pinned at black
status: done
deps: [nf-scene-correction/lomo100-cast-vs-ccr, nf-scene-correction/midtone-neutral]
tags: [new-flow]
summary: "the two-point slope as a scene-correction grade: a neutral film base where today's gains tint it"
done: 2026-10-13
---
# The roll white as a tilt pinned at black

> **From TASKS.md** (migrated 2026-10-14): the two-point slope as a scene-correction grade: a neutral film base where today's gains tint it; one of the two changes in the Lomography 100 "TR" arm. **Done 2026-10-13**: opt-in (`--white-balance-form tilt`); black neutral on 9 of 10 rolls, a small preference on low-key frames, the gain stays the default
>
> **Dependency note** (migrated 2026-10-14): filed 2026-10-12 from `lomo100-cast-vs-ccr`'s TR arm: the roll white as a tilt pinned at black

## Goal

Decide whether the roll's white correction should be a tilt instead of a gain:
- **the tilt:** zero at the film base, neutral at the roll's white;
- **today's gain:** shifts every tone by the same amount, which tints the film base.

If it should, make it supported.

## Design

Known (`lomo100-cast-vs-ccr`, 2026-10-12, Lomography 100; `../temp/lomo100-cast/`):

- **Arms.** The two-point slope (the colour-cast table of `docs/spike/poor-development.md`)
  as T. With the line off in both, gains → tilt (B → T) moved patch distance from
  neutral, lit cloth+wall / cloud, from 5.9 / 13.6 to 5.6 / 11.4. The tilt keeps the film base
  neutral (near-neutral pixels at the base: a\*, b\* −2.7, −6.8 today, +0.9, +0.8 under T).
  It leaves the lower midtones magenta (a\* +10…+12). Adding the line's red term (TR)
  removed that and helped lit cloth+wall (5.6 → 5.1), but cost cloud (11.4 → 12.7).
- **TR combined two changes.** TR was the tilt plus the line without its blue term.
  Dropping that term (TL → TR) moved lit cloth+wall 1.7 and cloud 2.5. The tilt (B → T)
  moved them 0.3 and 2.2. The blue term is [`midtone-line-terms`](midtone-line-terms.md).
- **The fixed-decode conflict.** The spike's two-point was a per-roll decode scale
  (`reconstruction.scale`). The fixed decode's rule is one global scale, never varied per
  stock, roll or frame ([`DENSITY_SCALE`]). The same tilt can be a scene-correction grade
  instead, a line in scene stops through zero at the base and the white's cast at the
  white, and the spike preferred the grade form for the line.
- **The spike's verdict.** On its rolls two-point helped 8 of 10, partially, and was
  superseded by the midtone line, never reviewed beside the line.
- **It shares the warm-white problem.** The tilt aims at the same roll p99 as today's
  gains, so a warm roll's white is wrong under both (the oracle arm O showed the gap).

Open:

- Grade form or decode form, and whether the grade form reproduces T.
- Whether the tilt and the midtone line are two terms or one line through the base.
- What becomes of the roll gains: `roll.white_balance`, the `roll.neutral_balance` switch
  (`--neutral-balance`) and `RollSection::applied_white_balance`. The midtone line applies
  only when the gains do, and its fade end is the gains' p99. Replacing the gains with the
  tilt would turn the line off silently, and TR needs both.
- Whether the neutral black survives the dark end (`fit_range.dark_end`, the floor by
  default) by eye: the user's lighter blacks are wanted. The arms were measured before the
  floor.

## How to Verify

- Patch casts and near-neutral black on every archive roll with marked neutrals, against
  today's chain.
- A whole-roll review on Lomography 100 and a poor-development roll, beside CCR.
- A drift-gate row; `docs/using-nc.md` if a key or flag changes.

## Dependencies

- [Hanten against SilverFast CCR on the Lomography 100 roll](lomo100-cast-vs-ccr.md) — the
  evidence and the arms
- [A midtone neutral measured per roll](midtone-neutral.md) — the line the tilt runs beside

[`DENSITY_SCALE`]: ../../../src/algo/fixed.rs
