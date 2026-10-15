---
title: Colour under a steeper midtone
status: done
deps: [nf-display-stages/midtone-contrast-toe]
priority: low
tags: [new-flow]
summary: suggest `--saturation 1.15` with the opt-in `--contrast 1.3` (a documented pairing, no code)
done: 2026-10-15
---
# Colour under a steeper midtone

> **From TASKS.md** (migrated 2026-10-14): **low priority**: whether saturation follows the opt-in `--contrast 1.3`
>
> **Dependency note** (migrated 2026-10-14): **low priority** split from `midtone-contrast-toe` 2026-10-11: whether saturation follows `--contrast 1.3`

## Goal

Decide whether saturation should follow the opt-in `--contrast 1.3`, once
[`midtone-contrast-toe`](../nf-display-stages/midtone-contrast-toe.md) holds its dark end,
or stay where it is.

## Design

Known:

- Colour stays at the roll's slope whatever `--contrast` is (`look.saturation` multiplies
  the colour's base slope; `contrast-on-luminance`; `docs/using-nc.md`, the look).

Settled (2026-10-15, review and the user;
[`docs/spike/midtone-contrast-colour.md`](../../spike/midtone-contrast-colour.md)):

- With colour held, ×1.3 keeps a frame's chroma (median per lightness band), except bright
  colour (below). Even so, **`--saturation 1.15` looked best beside it**, ahead of held
  and 1.3.
- It is a **documented pairing** (`docs/using-nc.md`), not a coupling in code: `--contrast`
  still leaves colour alone, and no render or recipe moved.
- Bright colour that ×1.3 pushes toward white loses chroma at the SDR top whatever the
  saturation. That depends on where ×1.3 places the highlights, so it is not this task's.

## How to Verify

- A review of renders at `--contrast 1.3` (under the default dark end), colour held at the roll's slope
  against colour following the contrast.

## Dependencies

- [More midtone contrast, the ends held by a toe and the
  shoulder](../nf-display-stages/midtone-contrast-toe.md) — the toe that holds
  `--contrast 1.3`, whose colour this decides
