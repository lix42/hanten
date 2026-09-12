import { describe, expect, it } from "vite-plus/test";
import {
  RAMP_CHROMA,
  RAMP_LIGHTNESS,
  RAMP_REFERENCE,
  inSrgbGamut,
  rampAt,
  rampColour,
  rampGradientStops,
  rampSpan,
} from "./ramp";

/** Red, green and blue channels of a `#rrggbb` string. */
function channels(hex: string): [number, number, number] {
  const n = Number.parseInt(hex.slice(1), 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

describe("cast ramp", () => {
  it("is neutral at zero, on both axes", () => {
    for (const axis of ["a", "b"] as const) {
      const [r, g, b] = channels(rampColour(axis, 0));
      expect(r).toBe(g);
      expect(g).toBe(b);
    }
  });

  it("points the way the axis does", () => {
    // a* is green below zero and red above; b* is blue below and yellow above.
    const [greenR, greenG] = channels(rampColour("a", -RAMP_CHROMA.a));
    const [redR, redG] = channels(rampColour("a", RAMP_CHROMA.a));
    expect(redR).toBeGreaterThan(greenR);
    expect(greenG).toBeGreaterThan(redG);

    const blue = channels(rampColour("b", -RAMP_CHROMA.b));
    const yellow = channels(rampColour("b", RAMP_CHROMA.b));
    expect(blue[2]).toBeGreaterThan(yellow[2]);
    expect(yellow[0]).toBeGreaterThan(blue[0]);
  });

  /**
   * The guard that earns this file. Raising a chroma past what sRGB holds does
   * not saturate the colour — it clamps one channel and *skews the hue*, which
   * looks plausible and is wrong. Measured at L* 65: green clips at 42.8 and
   * blue at 54.3, so `a*` may not exceed 41 nor `b*` 52.
   */
  it("keeps every ramp endpoint inside the sRGB gamut", () => {
    for (const [axis, chroma] of Object.entries(RAMP_CHROMA)) {
      for (const sign of [1, -1]) {
        const [a, b] = axis === "a" ? [sign * chroma, 0] : [0, sign * chroma];
        expect(inSrgbGamut(RAMP_LIGHTNESS, a ?? 0, b ?? 0)).toBe(true);
      }
    }
  });

  it("proves that guard can fail", () => {
    // Falsifiability: one step past the measured green limit is out of gamut.
    expect(inSrgbGamut(RAMP_LIGHTNESS, -44, 0)).toBe(false);
  });

  /**
   * The property the earlier version silently broke. It normalised by the
   * plotted range, so a frame whose worst band was `b* = +2` painted exactly
   * the same colour as one whose worst was `+20`.
   */
  it("paints the same value the same colour whatever else the frame contains", () => {
    const mild = rampAt("b", 2, rampSpan(-2, 2));
    const severe = rampAt("b", 2, rampSpan(-20, 20));
    expect(mild).toBe(severe);
  });

  it("proves that guard can fail", () => {
    // Falsifiability: range-normalising is what made the two agree wrongly.
    const rangeNormalised = (v: number, hi: number) => rampAt("b", (v / hi) * 20, 20);
    expect(rangeNormalised(2, 2)).not.toBe(rampAt("b", 2, rampSpan(-2, 2)));
  });

  it("is symmetric about neutral", () => {
    // With the old range-normalised map and bounds [-10, +18], b* = -5 came out
    // at chroma 26 against +5 at 14 — the same magnitude, twice the saturation.
    const span = rampSpan(-10, 18);
    const below = channels(rampAt("b", -5, span));
    const above = channels(rampAt("b", 5, span));
    const grey = channels(rampAt("b", 0, span));
    expect(Math.abs(below[2] - grey[2])).toBeCloseTo(Math.abs(above[2] - grey[2]), 0);
  });

  it("clamps past the reference rather than skewing the hue", () => {
    // A span wide enough to hold the value, and a value past its own span, must
    // both land on the endpoint rather than running out of gamut.
    expect(rampAt("b", 999, 20)).toBe(rampColour("b", RAMP_CHROMA.b));
    expect(rampAt("a", -999, 20)).toBe(rampColour("a", -RAMP_CHROMA.a));
  });

  it("widens only when a frame exceeds the reference", () => {
    expect(rampSpan(-3, 8)).toBe(RAMP_REFERENCE);
    expect(rampSpan(-30, 8)).toBe(30);
    expect(rampSpan(-3, 44)).toBe(44);
  });
});

describe("rampGradientStops", () => {
  it("runs from the top of the axis to the bottom, through neutral", () => {
    const stops = rampGradientStops("b", -10, 18, 20);
    expect(stops).toHaveLength(3);
    expect(stops[0]?.offset).toBe("0.0000");
    expect(stops[2]?.offset).toBe("1.0000");
    expect(stops[1]?.color).toBe(rampColour("b", 0));
    // Neutral sits where zero sits on the axis, not at the middle of it.
    expect(stops[1]?.offset).toBe((18 / 28).toFixed(4));
  });

  // The invariant the chart rests on: a stop's colour is its own value's colour
  // under the fixed span, so widening the axis moves stops without re-scaling
  // the ramp.
  it("gives every stop the colour of the value it sits at", () => {
    const [lo, hi] = [-10, 18];
    for (const stop of rampGradientStops("a", lo, hi, RAMP_REFERENCE)) {
      const value = hi - Number(stop.offset) * (hi - lo);
      expect(stop.color).toBe(rampAt("a", value, RAMP_REFERENCE));
    }
  });

  // Beyond the reference `rampAt` clamps, so a two-stop gradient would
  // interpolate over the clamped part and under-paint everything inside it.
  it("pins the reference itself when the axis runs past it", () => {
    const stops = rampGradientStops("b", -30, 40, 20);
    expect(stops.map((s) => s.color)).toContain(rampColour("b", RAMP_CHROMA.b));
    expect(stops.map((s) => s.color)).toContain(rampColour("b", -RAMP_CHROMA.b));
    expect(stops.length).toBeGreaterThan(3);
  });

  it("draws a single flat stop rather than dividing by zero", () => {
    expect(rampGradientStops("a", 0, 0, RAMP_REFERENCE)).toEqual([
      { offset: "0.5", color: rampColour("a", 0) },
    ]);
  });
});
