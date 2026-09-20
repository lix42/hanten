# Split `gamma` into calibration and look

## Goal

`gamma` is two things wearing one number. The half that linearizes the film (≈1.8,
i.e. 1/0.55) is a calibration and stays in the decode; **print contrast is a look and
moves to rendering**. After this task, changing how contrasty a picture is does not
touch the decode.

## Design

Today's single value bundles both — roughly the linearization plus ≈1.10× print
contrast at the exponential's 2.0 (the sigmoid's ≈2.07 is the same bundle at a
different setting). Splitting it means the decode keeps a fixed calibration and
rendering gains a contrast knob whose identity value reproduces today's look.

Two things make this less mechanical than it sounds:

- **`gamma` and `scale` are over-parameterized.** Only the products `gamma · scale_c`
  enter the curve, pinned by the convention `scale_r = 1` (design-update Part 1). So
  moving `gamma` moves the calibration unless `scale` moves with it; "measure `scale`
  from neutrals, `gamma` from a bracket" is one measurement split by a convention, not
  two independent ones. State which half of the product each stage owns, or the two
  epics will re-derive it differently.
- **The counterpart is not the same operator.** Rendering's contrast acts *after* the
  NC film RGB v1 3×3, so it matches the decode's `gamma` for neutrals and differs for
  saturated colour. The split is therefore not pixel-neutral in general, only on the
  neutral axis — say so rather than promising equality.

## Open questions

- **How the look-side contrast is spelled** — a standalone knob, or part of a
  CDL-style object alongside the per-channel grade. `nf-look` owns that spelling; this
  task needs only an identity value to land against, and may ship the contrast half as
  a placeholder the look epic replaces.
- **Does the decode's calibrated gamma stay user-reachable?** It is a flag and a
  recipe key today. If it stays, it is a calibration knob and should read as one.
- **The value 1.8 is a current pick**, expected to move with `scale` under
  `nf-calibration`; do not freeze the two independently.

## How to Verify

- With the look contrast at its identity value, a neutral patch renders where it does
  today at `gamma = 2.0`; any residual difference is stated and explained rather than
  tuned away.
- A test pins the over-parameterization convention (`scale_r = 1`), so a later change
  to either factor cannot silently double-correct.
- Changing the look contrast leaves the decode's output — `film-master` — untouched.
  That is the whole point of the split and is the falsifiable check.
- The four CI gates pass.

## Dependencies

- [The fixed, stock-agnostic decode](fixed-decode.md)
