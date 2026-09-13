import { describe, expect, it } from "vite-plus/test";
import { SYNTHETIC_METRICS } from "./fixture";
import { MIN_SCHEMA_VERSION, parseMetrics } from "./metrics";

/** The fixture, with one branch changed. */
function withDoc(mutate: (doc: Record<string, never>) => void): unknown {
  const copy = structuredClone(SYNTHETIC_METRICS) as Record<string, never>;
  mutate(copy);
  return copy;
}

describe("parseMetrics", () => {
  const parsed = parseMetrics(SYNTHETIC_METRICS);

  /**
   * The trap this parser exists for. `cast_by_tone_band` is a JSON object, so
   * its own key order is alphabetical — which is tone order scrambled, and which
   * plots a smooth, plausible, wrong curve. The fixture's keys are alphabetical
   * deliberately, so ordering by them instead of by `bands.names` fails here.
   */
  it("orders the cast by bands.names, not by the object's keys", () => {
    expect(parsed.cast.map((c) => c.band)).toEqual([
      "deep_shadow",
      "shadow",
      "low_mid",
      "mid",
      "high_mid",
      "highlight",
    ]);
  });

  it("proves the fixture would catch it", () => {
    // Falsifiability: the raw key order really is different from tone order.
    const raw = SYNTHETIC_METRICS as { color: { cast_by_tone_band: object } };
    expect(Object.keys(raw.color.cast_by_tone_band)).not.toEqual(parsed.cast.map((c) => c.band));
  });

  it("gives the same curve however the record's keys happen to be ordered", () => {
    // The strongest form of the guard: shuffle the object and the model is
    // byte-identical, because order comes from `bands.names` and nowhere else.
    const shuffled = withDoc((d) => {
      const doc = d as never as { color: { cast_by_tone_band: Record<string, unknown> } };
      const entries = Object.entries(doc.color.cast_by_tone_band).reverse();
      doc.color.cast_by_tone_band = Object.fromEntries(entries);
    });
    expect(parseMetrics(shuffled).cast).toEqual(parsed.cast);
  });

  it("skips a band the record omits rather than inventing one", () => {
    // `above_diffuse_white` is declared in bands.names but carries no pixels.
    expect(parsed.bands.names).toContain("above_diffuse_white");
    expect(parsed.cast.map((c) => c.band)).not.toContain("above_diffuse_white");
  });

  it("carries the sparse flag through", () => {
    const sparse = parsed.cast.filter((c) => c.sparse).map((c) => c.band);
    expect(sparse).toEqual(["highlight"]);
    for (const band of parsed.cast) {
      expect(band.sparse).toBe(band.fraction < parsed.bands.sparseBelowFraction);
    }
  });

  it("reads the reference bins and edges off the record", () => {
    expect(parsed.histogram.midGreyBin).toBe(49);
    expect(parsed.histogram.diffuseWhiteBin).toBe(100);
    expect(parsed.bands.lstarEdges).toEqual([15, 30, 45, 60, 75, 100]);
    // One fewer edge than names: the last band runs to the top of the range.
    expect(parsed.bands.lstarEdges.length).toBe(parsed.bands.names.length - 1);
  });

  it("keeps the fixture honest about what a real record guarantees", () => {
    const { histogram } = parsed;
    for (const [name, s] of Object.entries(histogram.series)) {
      expect(s.counts.length, name).toBe(histogram.bins);
    }
    // Every series, not just luminance: the fixture's channels were once
    // independently shaped, and blue summed to 101.2% of the frame — which this
    // assertion missed by looking at the one series where it happened to hold.
    for (const [name, s] of Object.entries(histogram.series)) {
      const summed =
        s.counts.reduce((a, b) => a + b, 0) + s.aboveRange + s.nonFinite + s.nonPositive;
      expect(summed, name).toBe(histogram.pixels);
    }
    // One series must exercise the above-range counter, so the chart's
    // "n% above L* x" notice has something to report in the demo.
    expect(Object.values(histogram.series).some((s) => s.aboveRange > 0)).toBe(true);
  });

  it("refuses a schema too old to carry a histogram", () => {
    expect(() =>
      parseMetrics(withDoc((d) => ((d as never as { schema_version: number }).schema_version = 1))),
    ).toThrow(new RegExp(`at least ${MIN_SCHEMA_VERSION}`));
  });

  it("refuses a cast band that is not a declared band", () => {
    expect(() =>
      parseMetrics(
        withDoc((d) => {
          const doc = d as never as { color: { cast_by_tone_band: Record<string, unknown> } };
          doc.color.cast_by_tone_band["shoulder"] = {
            mean_a: 0,
            mean_b: 0,
            fraction: 0.1,
            pixels: 1,
            sparse: false,
          };
        }),
      ),
    ).toThrow(/"shoulder".*not one of bands.names/s);
  });

  it("refuses a series whose bin count disagrees with the histogram", () => {
    expect(() =>
      parseMetrics(
        withDoc((d) => {
          const doc = d as never as {
            tone: { histogram: { series: Record<string, { counts: number[] }> } };
          };
          doc.tone.histogram.series["r"] = { counts: [1, 2, 3] };
        }),
      ),
    ).toThrow(/has 3 bins, but the histogram declares 200/);
  });

  it("names the offending path when a field is the wrong shape", () => {
    expect(() =>
      parseMetrics(
        withDoc((d) => {
          const doc = d as never as { tone: { histogram: { mid_grey_bin: unknown } } };
          doc.tone.histogram.mid_grey_bin = "49";
        }),
      ),
    ).toThrow(/tone\.histogram\.mid_grey_bin must be a finite number, got string/);
  });

  // The charts describe the region, not the frame. A set measures an inset
  // rectangle so the film holder stays out of the statistics, and a reader
  // told nothing would take the histogram for the whole picture.
  it("carries what part of the frame was measured", () => {
    expect(parsed.region.fractionWidth).toBeCloseTo(0.72);
    expect(parsed.region.fractionHeight).toBeCloseTo(0.72);
    expect(parsed.region.pixels).toBe(parsed.histogram.pixels);
  });

  it("refuses a record that does not say what it measured", () => {
    expect(() => parseMetrics(withDoc((doc) => delete doc["region"]))).toThrow(
      /region must be an object/,
    );
  });

  // A chart asks for channels by name, so a record missing one must fail *here*:
  // inside the component it would be a render error that replaces the whole page,
  // not one rendition's charts.
  it("refuses a record missing a series the charts draw", () => {
    expect(() =>
      parseMetrics(
        withDoc((doc) => {
          delete (doc as unknown as { tone: { histogram: { series: Record<string, unknown> } } })
            .tone.histogram.series["g"];
        }),
      ),
    ).toThrow(/tone\.histogram\.series is missing g/);
  });

  // The fixture stands in for a real record, so its region has to hold the one
  // invariant `resolve_region` guarantees: the rectangle's own area *is* the
  // pixel count. Asserted against the raw shape because the charted subset does
  // not carry the rectangle.
  it("has a fixture whose region is its own area", () => {
    const region = (SYNTHETIC_METRICS as { region: Record<string, number> }).region;
    expect(region["width"]! * region["height"]!).toBe(region["pixels"]);
    expect(region["pixels"]).toBe(parsed.histogram.pixels);
  });

  // A corner is not the centre, and the panel says "central" only when the
  // margins are equal — which it can only know if the offsets are parsed.
  it("carries where the measured rectangle starts, not only how big it is", () => {
    expect(parsed.region.fractionX).toBeCloseTo(0.14);
    expect(parsed.region.fractionY).toBeCloseTo(0.14);
    expect(parsed.region.fractionX * 2 + parsed.region.fractionWidth).toBeCloseTo(1);
  });

  // Every pixel of the measured rectangle falls in exactly one band, which is
  // the only shape the producer can emit — the same invariant `series()` holds
  // for the histogram, in the half no test used to cover.
  it("has a fixture whose tone bands partition the region", () => {
    const total = parsed.cast.reduce((sum, band) => sum + band.pixels, 0);
    expect(total).toBe(parsed.region.pixels);
    const fractions = parsed.cast.reduce((sum, band) => sum + band.fraction, 0);
    expect(fractions).toBeCloseTo(1, 4);
  });
});
