---
title: "A dark target: each frame's darkest 1 % placed at one depth"
status: done
deps: [nf-display-stages/midtone-contrast-toe]
tags: [new-flow]
summary: the dark target places a frame's darkest 1 % 5.5 stops below mid-grey, lifted or pushed
done: 2026-10-14
---
# A dark target: each frame's darkest 1 % placed at one depth

> **From TASKS.md** (migrated 2026-10-14): **done 2026-10-14** (`pipeline_version` 17): the dark target, `default`'s in place of the floor, places a frame's measured darkest 1 % 5.5 stops below mid-grey, lifted or pushed (a push capped at two thirds of the way to mid-grey). Chosen over 5 and 6 under `--placement frame`, kept under the hybrid. Whether `--dark-floor` retires is open
>
> **Dependency note** (migrated 2026-10-14): filed 2026-10-14: the floor only lifts, so a frame's black sits where the placement leaves it (4.26 stops under under `frame`); a target moves it either way

## Goal

Decide whether a frame's black should be **placed** at a depth rather than only held
above one, and at what depth. Today nothing pulls a frame's shadows down: under
`--placement frame` every ordinary frame's darkest 1 % renders about 4.26 stops under
mid-grey (L\* ≈ 8.5, 8-bit ≈ 24), a visible gap at the histogram's left end.

## Design

Known (2026-10-14, 2026-10-12 Gold200, 36 frames):

- The 4.26 is the placement plus reinhard, not the picture: the frame white at 4.462
  look stops less `SPAN_LOOK_STOPS` 9.049 puts the dark end at −4.59 (the spike aimed at
  −5, with its white at 4.049), and reinhard's mid-grey-preserving gain (≈ 1.22 near
  black) lifts it 0.3 stop. The 4.5 floor only lifts, so it never acts.
- The darkest 0.01 % sits only ≈ 0.3 stop under the darkest 1 %: the film's own toe.
- Display black (`--display-black`) reaches deeper but follows the film base, which
  renders 4.0–4.7 stops under on this roll, so frames move unevenly.
- CCR's darkest 1 % on this roll sits 7.7 stops under mid-grey (median; L\* ≈ 1),
  deeper than any arm reviewed.

Decided (2026-10-14, by review of that roll): `--dark-target STOPS` (`fit_range.dark_end`
`{"target": STOPS}`), the floor's reference moved either way, a push capped at two thirds
of the way to mid-grey (slope ≤ 2) so high-key frames keep light shadows. **5.5 is
`default`'s dark end** (`pipeline_version` 17), chosen over 5 and 6 under
`--placement frame`, and kept under the hybrid, where the cap holds this roll at 3.4.

Open:

- Whether `--dark-floor` retires now that nothing defaults to it.
- The hybrid on this roll renders the darkest 1 % only 2.0 stops under mid-grey, with
  its median half a stop above and ≈ 9 % of each frame at white: a placement question,
  not the dark end's.
- Shadow noise at the deeper black was judged by eye only, on one roll.

## How to Verify

- A review set on whole rolls, under `--placement frame` and the hybrid, today's floor
  beside the target at the candidate depths, with the darkest 1 % and shadow noise
  measured beside each arm.

## Dependencies

- [More midtone contrast, the ends held by a toe and the
  shoulder](midtone-contrast-toe.md) — the dark floor and its curve, which the target
  reuses
