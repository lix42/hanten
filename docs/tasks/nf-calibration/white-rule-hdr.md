# The white rule in HDR

## Goal

Review the roll's white placement on an HDR rendition, so its values can stop being
provisional. Every round that chose and checked it was judged on SDR renders. Since
2026-10-09 that placement is [`span-roll-slope`](span-roll-slope.md)'s span slope and
display white target, which may replace `roll-white-rule`'s cap, floor and target below.

## Design

What is known:

- **Where the white sits also decides the HDR headroom.** Everything above the roll's
  white is what the HDR branch lifts toward its peak. `nf-reconstruction/anchor-rule`
  estimated that a per-frame p97 white leaves about a stop above it on a typical
  frame. The rule places the roll's white between +1.5 and +2.0 scene stops
  above mid-grey, so how much headroom is left varies by roll and by frame.
- **Fit range stays reinhard** (`nf-display-stages/parametric-shoulder`, 2026-09-27).
  The HDR branch is reinhard plus the peak lift, `Y′ = r(Y)·(1 + (P − 1)·s(Y))`. Fit
  range's output is identical to SDR's below diffuse white (`chain::render_pair`). The
  renditions still differ there where the SDR cube binds a saturated colour that HDR
  keeps (`chain::contract`), and in the destination's gamut and encoding. So the tone
  differs from the reviewed SDR only above white, but colour can differ below it too.
- **The rule's knobs**: the cap (+2.0), the floor (+1.5), the saturation margin
  (`saturation-margin`'s), and a white target 0.15 stop above diffuse white that was
  "a little better" in some SDR cases. Headroom is `--display-tone-headroom`.

Open:

- **Which HDR rendition to review.** `--range hdr` alone writes the per-channel
  gain-map JPEG (`nf-destinations/gain-map-destination`), and
  `--range hdr --transfer pq|hlg` writes a Rec.2100 signal. Viewing HDR needs an HDR display and a viewer that keeps the HDR signal
  (`analysis/comparison-review-tooling` records that `sips` destroys gain maps).
- What to measure alongside the review: headroom above the roll's white per frame, the
  share of pixels the lift reaches, and HDR samples past the peak.
- **The gamut map's share on the HDR branch.** `docs/reports/gamut-map-share.md`
  measured SDR only and found the map near-inert there under reinhard. It handed the
  HDR branch to `fit-range` / `parametric-shoulder`, and neither measured it. Measure
  it here, with the same probe approach.
- Whether the clamped frames (rendered at the cap's contrast) need anything different
  in HDR.
- What `span-roll-slope`'s display white target means on HDR, and how much headroom is
  left once the white moves toward L\* 91.

## How to Verify

- A review set on HDR renders across the nine rolls (clamped and ordinary frames) with
  the rule's contrast, and a recorded verdict: the cap, floor and target stand or move.
- If a value moves, `measure-roll` and its tests follow it, and the SDR verdicts are
  re-checked.

## Dependencies

- [`measure-roll` places the roll's white](roll-white-rule.md) — the rule under review
- [Does a parametric shoulder beat reinhard?](../nf-display-stages/parametric-shoulder.md)
  — settles the operator the HDR rendition is judged under
- [A measured roll exposure](roll-exposure.md) — sets the level the rule is reviewed at
  (the white stays measured at exposure 0, and the exposure moves where it renders)
- [The roll's slope from its span](span-roll-slope.md) — the SDR white target this
  reviews on HDR
