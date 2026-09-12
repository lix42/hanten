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
});
