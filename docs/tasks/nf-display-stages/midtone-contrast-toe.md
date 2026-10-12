# More midtone contrast, the ends held by a toe and the shoulder

## Goal

Hold the dark end up wherever it sinks into black, with a toe beside reinhard's shoulder:
under a steeper slope, first the opt-in `--contrast 1.3` the probe settled on, and in the
default's own deep tails. The default slope stays as it is.

## Design

Known:

- The probe (`docs/spike/midtone-contrast-probe.md`): the default stays at ×1; ×1.29 was
  liked as an opt-in, judged with its shadows sinking — its darkest 1 % renders near L\* 6
  instead of 12, and the dark end takes an estimated median 58 % of the extra stops. The white is
  already held by the shoulder.
- Earlier toes pushed black **down** to place it (`parametric-operator`; the old
  sigmoid's toe in `algo/reconstruction-render-curve-split`, dropped for costing black
  depth). This toe holds shadow detail **up** after a steeper midtone; that is untested.
- Two cases `nf-calibration/hybrid-slope-bounds` hands here, where content passes the
  span's targets with no slope at fault: the tails beyond a frame's own ends (its darkest
  and brightest 1 %), and a span wider than ~7.3 stops, where the floor slope (1.237) stops
  compressing. A steep frame's tail at ×1 also sinks (1719).

Open:

- The toe's shape, and where it sits: fit range is the natural stage, and it must keep
  below diffuse white identical across peaks.
- How it coexists with display black.
- Whether ×1.29 with a toe compares differently with ×1.
- Noise on thin frames: a steeper midtone raises grain in stops, and the maximum slope
  binds more often — `nf-calibration/short-span-protection` re-checks it after this task.

## How to Verify

- A review of the toe against the straight slope at matched midtone contrast (×1 and
  ×1.29), with shadow noise and the share of pixels near black measured, not only judged.
- Then `--contrast 1.3` under `--placement roll`, where low- and high-key frames are
  pushed further than under the hybrid; then `frame`, whose pinned white it moves.

## Dependencies

- [Does more midtone contrast win?](midtone-contrast-probe.md) — the opt-in contrast this
  toe holds, and how far its dark end sinks
