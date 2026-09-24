# How much highlight desaturation is the gamut map's

Produced by `nf-display-stages/gamut-map-share`, 2026-09-23, against the shipped SDR
path (Display P3). Probe, scripts and raw numbers in `../temp/gamut-map-share/`
(uncommitted — the frames are the user's photographs; `probe.diff` is the throwaway patch).

## The answer

**Where it matters, almost none.** Across 92 frames on four rolls and 41 hand-marked
white patches, the gamut map changes **no marked patch** under `sigmoid-knees`, the
desaturation spike's control, or `path-to-white`'s hand-set C and D — the `none` and
`reinhard` tones. On the top 3% of pixels it removes C\* 0–2.6 on average per roll, and
that comes from a handful of frames rather than a general effect. Under the `shoulder`
tone it is a different story (below).

- **The knee'd sigmoid's clean whites are not the gamut map's.** Under
  `--preset sigmoid-knees` the map touches **0.00%** of top-end pixels on every roll. Of
  Appendix F's two surviving candidates, what remains is the per-channel shoulder.
- **`path-to-white` has nothing to double up with** under the configurations it is built
  and tuned in. The map is incidental there.
- **Every limit it hit was the ceiling** — the top of the display cube — never the floor.
  No top-end colour on these rolls is outside P3's primaries.

## Method

A throwaway `#[ignore]`d probe ran each frame through the shipped reconstruction, print
controls and `sdr` arithmetic, and read every pixel on both sides of `gamut_map` **in
float**. That is what makes "off" mean *unmapped*: a patch reverting the map in the
binary would have handed its overshoot to the u16 encode, whose per-channel clip is a
gamut policy of its own. A patch to the binary was therefore never an option for the
measurement. The probe's mapped output was checked **bit-identical** to `sdr::render`
on every render the shipped path accepts (403 of 403).

Chroma is CIELAB C\* on linear P3, whose linear segment keeps an unmapped out-of-gamut
sample defined. Two selections, the spike's lesson being that aggregates hide the
revealing case:

- **Marked patches** — the patch's median RGB. 12 on `2026-09-18-Gold200` (the
  desaturation spike's), 4 on `2026-09-09-Ektar100` and 8 on `2026-09-11-Portra400`
  (the neutral-patch page), 17 on `2026-09-20-Portra400` (marked for this task). Five
  earlier marks were lost to frames since pruned from the assets.
- **Top 3%** of the interior (10% inset) by rendered luminance, taken from one arm and
  held fixed across the arms compared against it.

Arms, with each roll's measured base: `sigmoid-knees`; the same with
`--sigmoid-shoulder 0`; the spike's control (exponential, white at each frame's red p97,
reinhard); `path-to-white`'s hand-set **C** (`--anchor-mid-offset 0.62`,
`gamma = 0.7447 / (W − 0.62)` from the roll's red p97 `W`, reinhard); and **D**, C with
gamma capped at 3.18, the top of the outside converters' range. The cap binds only on
Gold200 (C asks 4.14 from `W` = 0.800 as rounded; `white-placement.md` prints 4.15); where D's ceiling belongs is `nf-calibration/anchor-comparison`'s.

## Numbers

Top 3%, mean C\* the map removes per frame (worst frame in brackets):

| roll | frames | knees | control | C | D |
|---|---|---|---|---|---|
| 2026-09-18-Gold200 | 35 | 0.00 | 0.00 (0.05) | 0.63 (8.7) | 0.00 (0.02) |
| 2026-09-09-Ektar100 | 12 | 0.00 | 2.03 (24.2) | 0.00 (0.01) | — |
| 2026-09-11-Portra400 | 11 | 0.00 | 2.63 (28.5) | 1.47 (15.8) | — |
| 2026-09-20-Portra400 | 34 | 0.00 | 0.65 (16.0) | 1.07 (13.4) | — |

Frames with more than C\* 1 removed under C: 3 / 0 / 1 / 4. The means are those frames,
each a bright region pressing on the ceiling — never a marked white.

**The marked patches are unchanged** under knees, control, C and D on all four rolls:
across 135 patch-by-arm readings the largest C\* the map moved is 0.001.

## What does converge chroma heavily, and why it is not a share

- **The default `shoulder` display tone.** C with no `--display-tone` renders under
  `shoulder`, and there the map removes C\* 5.3–13.8 per roll on average (worst 52) and
  touches 13–63% of top pixels. It moved 4 of the 41 marked whites, three of them by
  C\* 25–28 (1880's two clouds, 1811's white cloth, all pushed to L\* 100). This is the
  flattening the spike saw from
  `--display-tone shoulder`: the Hermite plateau puts luminance at the cube's top, where
  the only in-gamut colour is white. `path-to-white`'s hand-set spelling left the tone
  unstated and so meant this render; it needs `--display-tone reinhard`.
- **The knee'd render without its shoulder.** `--sigmoid-shoulder 0` under
  `sigmoid-knees` is refused on every frame (18–89% of top pixels above display white),
  and in float the map drives those pixels to exact grey. The 2x2 that ordering implies
  would credit the map with 93–130% — not a share of anything shippable, since the
  shipped renderer refuses that render. The honest reading is that the shoulder does the
  convergence and the map never gets a pixel to act on.
- **Reinhard above display white** collapses a pixel to exact neutral (the ceiling
  follows its luminance, so no chroma fits). Only ~21 top-end pixels reached it, on three Gold200 frames under C (≈2e-4% of top
  pixels); none on the other rolls or arms.

## What this does not settle

- **The shoulder's own convergence is not separated from its luminance compression.**
  Removing it lifts the brightest marked whites from L\* ~95 to as far as 155, so knees
  against knees-without compares two brightnesses. The map's absence is measured; the shoulder's share is what
  remains by elimination, not by a matched render.
- **HDR and the gain map.** Their copies of the map carry different ceilings
  (`LINEAR_HEADROOM`, the stored base) and were not measured. `path-to-white` runs
  pre-branch, and the spike measured the SDR base.
- **Dim frames under C.** All eight Portra 0911 marked whites render at L\* 24–53 under C,
  since one roll white darkens a dim frame — `white-placement.md`'s finding, not this one's.

## Decisions

- **No gamut-map off switch, on either chain.** Where `path-to-white` operates the map
  hides no cast, so "off" would reveal nothing. Should one ever be wanted, `fit-gamut`
  records what it has to mean: an unmapped float destination, never a per-channel clip.
- **The map needs no instruction to shrink, under `none` and `reinhard`.** `path-to-white`'s "what
  should shrink instead is the gamut map's incidental share" is a remark, not work for
  `fit-gamut`: under `reinhard` and `none` in SDR the share is already near zero. This
  does not transfer to a fit-range operator that plateaus near display white — the
  `shoulder` result shows the map then acts hard — nor to the HDR branch; both are for
  `fit-range` / `parametric-operator` to re-check.
