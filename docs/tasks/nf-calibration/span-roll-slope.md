# The roll's slope from its span, and where its white lands

## Goal

Replace the per-roll placement's white-to-midtone slope with one set by the roll's span:
the roll's white goes to a white target on the display, its dark end to a dark target,
and the slope follows. Settle the SDR white target and where the placement anchors as
part of it — a span slope cannot be defined without them. Absorbs `display-white`.

## Design

Known (`docs/spike/poor-development.md`, 2026-10-04/05):

- **The span slope** runs from the roll's white to the 10th percentile of its frames'
  darkest 1 %. On ten rolls it lands at 1.57–1.75, where today's white-to-midtone slope
  spans 1.05–1.57; thin rolls rise most. The spike's form held it within the white cap's
  minimum slope and a global maximum, which this task introduces at the provisional 3.5
  (`hybrid-slope-bounds` tunes it).
- **It is the floor of the envelope hybrid** (`envelope-hybrid-placement`), and the
  hybrid keeps the roll's brightest and darkest points where this placement puts them, so
  the white chosen here is the hybrid's too.
- **The white target** (from `display-white`): on SDR diffuse white renders at L\* 79 and
  every rule takes a frame's p97 as its white, so the band above is mostly empty. After the
  target-0 exposure the roll's own white already lands near L\* 84–100; the low figures
  are the median frame's bright end.
  Reviews of steep placements preferred the white near L\* 90–92 (L\* 93 sat ~0.6 stop
  brighter than CCR in the midtones); the spike's targets put it at about L\* 91.
- **The anchor**: with exposure at the midtone (`level-target-zero`), the span slope puts
  the roll's whitest point at L\* 97.7, near clipping. Pinning the white at L\* 91 under
  the white-to-midtone slope flattened thin rolls (09-11 1.65 → 1.05), so the white and
  the brightness target are set together.
- Whites are measured after the roll's colour correction
  (`nf-scene-correction/midtone-neutral`), which moved them and rendered the reviewed
  rolls a little darker on the expectation that this work brightens white.
- `nf-display-stages/parametric-shoulder` reviewed a brighter white with mid-grey pinned
  and it lost, at the old −0.6 target and roll level only.
- Display black is still needed: a span slope sets the distance between the ends, not
  where the dark end sits.

Open:

- Where the white target lives (a placement value, not a new curve), and how it combines
  with the level target: a lower exposure, or a cap on the white.
- What becomes of the roll white rule's cap and floor (+2.0 / +1.5), and of
  `roll.white_stops`, once the slope comes from the span.
- Whether the dark end is the darkest 1 % or 5 %.
- What `direct` and the no-roll fallback slope do (`no-roll-defaults`).
- The thin and small lifts against the steeper base: today's thin slopes (1.78–1.90 on
  09-29) sit just above the span slope, so which frames a lift helps changes
  (`thin-lift-confirmation`).

## How to Verify

- `measure-roll` on the archive rolls: the slopes, and where each roll's white and dark
  end land.
- A review at this placement comparing today's white with one or two display targets, on
  a good, a thin and a poor roll.
- A drift-gate row; `docs/using-nc.md` describes the placement.

## Dependencies

- [Raise the roll's brightness target to 0](level-target-zero.md) — the exposure the
  white is set together with
- [A midtone neutral measured per roll](../nf-scene-correction/midtone-neutral.md) — the
  whites the span is measured from
- [Contrast on luminance, saturation its own
  setting](../nf-look/contrast-on-luminance.md) — so the steeper roll slope is not also a
  more saturated one
