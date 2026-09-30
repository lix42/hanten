# A per-frame level trim

## Goal

A small, bounded exposure trim per frame around the roll's measured exposure, so a
low-key frame renders a little brighter and a bright frame a little darker than one
exposure for the whole roll allows. The roll's exposure stays the anchor, and the recipe
and report show what each frame got.

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

Open:

- **What keys it**: the frame's level (log-average luma) against the roll's median, its
  white, or both. Round 2's split followed the frame white.
- **The bound**, and whether the trim is on by default or opt-in.
- **Where it lives**: `roll.frames` entries written by `measure-roll`, beside the clamps,
  or a rule applied at render time from a measured per-frame value. Either way, a
  `roll.frames` exposure has to be pinned as a total or as a delta on the roll's, which
  `thin-frame-lift` also has to settle. Settle it once for both.
- **Colour.** Whether a trimmed low-key frame shows a cast that one roll exposure hid.

## How to Verify

- A review round on normal and dark rolls: the trimmed renders against the roll exposure
  alone, on the frames round 2 split.
- A night scene and a bright scene on the same roll stay within the bound, and the report
  names each frame's trim.
- A recipe without per-frame trims renders byte-identically to the build before.

## Dependencies

- [A measured roll exposure](roll-exposure.md) — the anchor the trim is measured around,
  and the per-frame level it can reuse
