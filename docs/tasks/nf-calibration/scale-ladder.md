# A `density.scale` ladder, before the calibration frames exist

## Goal

Find the best per-channel `density.scale` the **exponential** decode can be given from
the scans we already have, and record what that method can and cannot settle. Runs
against today's binary, so it needs none of the migration and no colorchecker.

## Design

`[1, 0.84, 0.73]` was fitted for the sigmoid-era default and carries a documented
split (`src/types.rs:698`): blue is solid — every roll measured wants 0.68–0.78 —
while **green splits by scan date** (July 0.86–0.90, September ~0.77, consistent with
a developer change), so one value fits neither group. The new decode would inherit it
by inertia.

- **A ladder, not a fit.** Render `sigmoid-flat` (which *is* the exponential —
  `toe = shoulder = 0`, bit-exact) across candidate scales spanning the green split,
  blue held near its converged value, on the Appendix E frames.
- **`sigmoid-knees` rides along as a comparison target, not ground truth.** The
  verdict that made it the reference was made by eye, and the user is not sensitive
  to light green — the axis where the calibration is weakest.
- **Green is measured, not judged**, for that reason. The eye decides red/blue and
  overall preference; green is read off neutral surfaces with `nctool metrics`.
- **SDR, gain map stripped** — the scale sits before the SDR/HDR branch so both
  renditions inherit it, and the verdict it is compared against was made on SDR.
  `sigmoid-knees` has no HDR headroom anyway (its gain map decodes 1.0x).
- Whether any scale reaches knees' whites is the **by-product that decides
  [path to white](../nf-look/path-to-white.md)**: if one does, the shoulder was
  masking a calibration error and the operator is an optional look; if none does, it
  is load-bearing and must exist before the default can flip.

## Open questions

- What counts as a neutral surface without a colorchecker. The 31 hand-marked patches
  are *assumed*-neutral scene content, which is the whole reason the shoot exists.
- Whether the ladder is judged per roll or pooled — the green split is per scan date,
  and pooling is what produced a value fitting neither group.
- Whether a residual this leaves is a decode error or belongs to the look stage's
  per-channel grade.

## How to Verify

A written answer in `docs/progress/nf-calibration.md`: the candidate scale with its
evidence, the measured green residual, and an explicit statement of what the method
could not settle. An HDR check on the winning scale alone — not a ladder — confirming
the gap to SDR is within acceptable scope rather than matched.

The value is **provisional by construction** and does not move a default on its own;
[the neutrality gate](neutrality-gate.md) is what confirms it against known-neutral
frames.

## Dependencies

None — it runs against today's binary, deliberately, so that it can run first.
