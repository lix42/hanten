# A midtone neutral measured per roll

## Goal

Remove the cast a poor development leaves in the midtones, which a single roll white
balance cannot: the cast grows away from the roll's p99, where the white balance is
exact. A scene-correction step, measured once per roll by `measure-roll` and carried in
the recipe, on by default.

## Design

Known (`docs/spike/poor-development.md`, 2026-10-02/06):

- **The method is the joined line.** Per band of brightness, each frame votes the densest
  cluster of its colour ratios after the roll white balance; the roll's value per band is
  the median vote; a weighted line through the bands gives red and blue a correction
  against scene stops. It applies in full up to 1 stop below the roll's white, fades to
  zero at the white, and is flat outside the voted bands. Luminance is restored per pixel.
- **Review**: no bad frame on any reviewed roll (09-29, 09-20 poor; 09-18, 09-14 good),
  best most often. 09-29's neutral-patch cast 21.4 → 3.4 (CCR 5.8).
- **The switch is a data floor, not a detector**: on unless the roll has fewer than 10
  frames or too few voted bands. A correction in `taste-vs-quality`'s terms, with
  `auto` / `on` / `off` and the user's override.
- **Two passes over the same samples, no loop**: colour first (white balance and the
  line), then each frame's white measured on the corrected samples. Today the white is
  measured before white balance, so any colour correction moves every frame's placement
  (09-20's roll white 1.68 → 1.92 under one form).
- **Replaces nothing**: the decode's fixed scaling and the roll white balance stay. The
  line measures what they leave, is off without enough data, and is flat in shadows and
  highlights.
- The spike's code (`spike/midtone-guard`, never merged) is evidence, not a starting
  point.

Open:

- Fit range (every voted band, or below the fade) and fade width: wait for ColorChecker
  frames (`analysis/calibration-frame-capture`). Ship the spike's values until then.
- The beach case (a roll dominated by one scene colour) and warm light filling a frame's
  midtones (09-20 1883): a guard, a tint gate, or a documented turn-down.
- Whether the white rule's p97, cap +2.0 and floor +1.5 still hold once whites are
  measured after correction; they were tuned before it.
- The recipe and report shape, and how a frame override or a stated `--white-balance`
  combines with the line.
- Highlight desaturation as a final cleanup on top.

## How to Verify

- `measure-roll` on the ten archive rolls writes a line on every roll with ≥ 10 frames,
  and none on the three- and four-frame rolls; the reviewed rolls' renders match the
  spike's to within its patch numbers.
- A review of the moved whites (two-pass measurement) on a cast roll and a good roll.
- A drift-gate row; `docs/using-nc.md` states the step, its switch and its fallback.

## Dependencies

- [A roll-level white balance](roll-white-balance.md) — the measurement this extends
- [Separate taste from quality](../nf-calibration/taste-vs-quality.md) — the class and
  switch shape of an automatic adjustment
