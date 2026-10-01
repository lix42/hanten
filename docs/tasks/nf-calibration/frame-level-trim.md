# A per-frame level trim

## Goal

A small, bounded exposure trim per frame around the roll's measured exposure, so a
low-key frame renders a little brighter than one exposure for the whole roll allows (the
review found bright frames want no trim, so it only lifts). The roll's exposure stays the
anchor, and the recipe and report show what each frame got.

## Design

What is known:

- **One exposure per roll has gone as far as it can.** In `roll-exposure`'s round 2
  (`../temp/roll-exposure/review-r2.json`), a target 0.3 stop brighter (−0.3 against −0.6)
  split 8 to 10 **by frame, not by roll**. Low-key frames (frame white −1.5 to +0.3) wanted
  the brighter target, on normal rolls too (09-18 1798 and 1774, 09-20 1886, 07-15 989).
  Bright frames (+0.9 to +2.6) wanted −0.6. So the trim is around +0.3 EV either way.
- **It is not `thin-frame-lift`.** That task is opt-in and lifts a frame far thinner than
  its roll to a full print with a steeper slope. This one is small and neutral, and applies
  to every frame.
- **The per-frame failure is known.** A frame's own statistics read a night scene as
  under-exposure, which retired per-frame auto white balance
  (`nf-scene-correction/roll-white-balance`). A bound is what keeps the trim from doing
  the same.

Settled 2026-09-30 by two review rounds (details in `docs/progress/nf-calibration.md`,
`## frame-level-trim`):

- **The key** is where the frame's white renders after the roll's exposure. The white
  alone re-did the roll's exposure on thin rolls; the level against the roll's median
  split its verdicts evenly.
- **A lift only**, up to +0.3 EV: bright frames wanted no trim, not a darker one.
- **On by default**, with two opt-outs: one when measuring, one when rendering, so a
  preview can turn it off without re-measuring.
- **Where it lives:** `roll.frames` entries, as a **delta** on `roll.exposure`, which
  binds `thin-frame-lift` too.
- **Colour:** no cast reported on the lifted frames.

## How to Verify

- A review round on normal and dark rolls: the trimmed renders against the roll exposure
  alone, on the frames round 2 split.
- A night scene and a bright scene on the same roll stay within the bound, and the report
  names each frame's trim.
- A recipe without per-frame trims renders byte-identically to the build before.

## Dependencies

- [A measured roll exposure](roll-exposure.md) — the anchor the trim is measured around,
  and the per-frame level it can reuse
