/**
 * A synthetic metrics record, for the `/charts` demo and the unit tests.
 *
 * Hand-authored rather than measured, for two reasons. It contains no trace of
 * anyone's photographs, so it is safe to commit; and it can carry the cases real
 * data does not reliably contain — a `sparse` band, a band absent altogether,
 * non-zero `above_range` — which is exactly where an encoding fails quietly.
 *
 * Held in the **raw `snake_case` shape**, so the demo and the tests both go
 * through `parseMetrics` rather than around it.
 *
 * **The keys of `cast_by_tone_band` are deliberately alphabetical**, which is how
 * `JSON.parse` hands back a real record too. If a chart ever orders by them
 * instead of by `bands.names`, this fixture draws the wrong curve and
 * `metrics.test.ts` says so.
 */

const BINS = 200;

/**
 * The measured rectangle, and the pixel count every series partitions.
 *
 * `pixels` is the region's own area in a real record — `tone.histogram.pixels`
 * and `region.pixels` are the same number, both counted over the rectangle that
 * was measured — so the three are derived from one place here rather than
 * written out and left to disagree.
 */
const REGION = { x: 264, y: 184, width: 1359, height: 945 };
const PIXELS = REGION.width * REGION.height;

/** A lump of tone centred on `centre`, so the demo looks like a photograph. */
function bump(centre: number, spread: number): number[] {
  return Array.from({ length: BINS }, (_, i) => Math.exp(-((i - centre) ** 2) / (2 * spread ** 2)));
}

/**
 * One series, scaled so it partitions `PIXELS` exactly.
 *
 * `metrics.py` partitions every block, so a real record always satisfies
 * `sum(counts) + above_range + non_finite + non_positive == pixels`. Independently
 * shaped gaussians do not: before this, blue summed to 101.2% of the frame — a
 * shape no record can produce, which the charts then drew.
 */
function series(centre: number, spread: number, aboveRange = 0) {
  const raw = bump(centre, spread);
  const rawTotal = raw.reduce((a, b) => a + b, 0);
  const budget = PIXELS - aboveRange;
  const counts = raw.map((c) => Math.round((c / rawTotal) * budget));
  // Rounding leaves a few counts either way; settle them on the modal bin.
  const modal = counts.indexOf(Math.max(...counts));
  counts[modal] = (counts[modal] ?? 0) + (budget - counts.reduce((a, b) => a + b, 0));
  return { counts, above_range: aboveRange, non_finite: 0, non_positive: 0 };
}

const LUMINANCE = series(52, 15);
const RED = series(56, 15);
const GREEN = series(52, 14);
// Blue sits lower and carries a few samples past the top of the range, which is
// the only series exercising the `above_range` counter.
const BLUE = series(41, 16, 1200);

export const SYNTHETIC_METRICS: unknown = {
  schema_version: 2,
  file: "synthetic.jpg",
  // An inset rectangle, as a review set's records carry: the holder and the
  // rebate are kept out of the statistics, so the charts describe this region
  // and not the whole frame.
  region: {
    ...REGION,
    pixels: PIXELS,
    // The *requested* fractions, as `resolve_region` records them; the pixel
    // rectangle above is those fractions of a 1888x1312 frame, rounded.
    fraction: { x: 0.14, y: 0.14, width: 0.72, height: 0.72 },
  },
  bands: {
    domain: "cielab_lstar",
    names: [
      "deep_shadow",
      "shadow",
      "low_mid",
      "mid",
      "high_mid",
      "highlight",
      "above_diffuse_white",
    ],
    lstar_edges: [15, 30, 45, 60, 75, 100],
    sparse_below_fraction: 0.001,
  },
  tone: {
    histogram: {
      domain: "cielab_lstar",
      bins: BINS,
      bin_width_lstar: 1,
      lstar_range: [0, 200],
      mid_grey_bin: 49,
      diffuse_white_bin: 100,
      pixels: PIXELS,
      series: { luminance: LUMINANCE, r: RED, g: GREEN, b: BLUE },
    },
  },
  color: {
    // Alphabetical, as JSON.parse yields. Tone order is deep_shadow, shadow,
    // low_mid, mid, high_mid, highlight — nothing like this.
    // The six bands **partition the region exactly** — 1,284,255 pixels, the same
    // number `tone.histogram.pixels` carries — because that is the only shape
    // `metrics.py` can emit: every pixel of the measured rectangle falls in one
    // band. A fraction list summing to 1.0004, as this once did, is a record no
    // producer could write. `above_diffuse_white` is absent rather than zero,
    // which is the one legitimate way a band goes missing.
    cast_by_tone_band: {
      deep_shadow: { mean_a: 0.4, mean_b: 2.2, fraction: 0.021, pixels: 26_969, sparse: false },
      high_mid: { mean_a: -2.4, mean_b: 3.9, fraction: 0.118, pixels: 151_542, sparse: false },
      // Under `sparse_below_fraction`: drawn hollow, never at full weight.
      highlight: { mean_a: -3.1, mean_b: -6.8, fraction: 0.0004, pixels: 514, sparse: true },
      low_mid: { mean_a: -0.9, mean_b: 11.4, fraction: 0.271, pixels: 348_033, sparse: false },
      mid: { mean_a: -1.8, mean_b: 14.2, fraction: 0.5116, pixels: 657_025, sparse: false },
      shadow: { mean_a: 0.1, mean_b: 6.1, fraction: 0.078, pixels: 100_172, sparse: false },
      // `above_diffuse_white` is absent on purpose: a band with no pixels is
      // omitted by the producer, and the charts must simply not draw it.
    },
  },
};
