import { describe, expect, it } from "vite-plus/test";
import {
  extent,
  histogramOutline,
  niceStep,
  paddedBounds,
  linearScale,
  plotArea,
  polyline,
  populationRadius,
  slotCentres,
  ticks,
} from "./scale";

describe("linearScale", () => {
  it("maps the ends of the domain onto the ends of the range", () => {
    const s = linearScale([0, 10], [100, 300]);
    expect(s(0)).toBe(100);
    expect(s(10)).toBe(300);
    expect(s(5)).toBe(200);
  });

  it("inverts for a y axis, so larger values sit higher on screen", () => {
    const y = linearScale([0, 10], [20, 220], true);
    expect(y(0)).toBe(220);
    expect(y(10)).toBe(20);
    expect(y(10)).toBeLessThan(y(0));
  });

  it("draws a flat line rather than NaN when every value is the same", () => {
    const s = linearScale([7, 7], [0, 100]);
    expect(s(7)).toBe(0);
    expect(Number.isNaN(s(7))).toBe(false);
  });
});

describe("slotCentres", () => {
  it("puts the first and last slot on the plot edges", () => {
    expect(slotCentres(6, 0, 500)).toEqual([0, 100, 200, 300, 400, 500]);
  });

  it("centres a single slot instead of collapsing it onto the left edge", () => {
    expect(slotCentres(1, 0, 500)).toEqual([250]);
  });

  it("has nothing to place for no bands", () => {
    expect(slotCentres(0, 0, 500)).toEqual([]);
  });
});

describe("ticks", () => {
  it("includes zero exactly, because the neutral line is read against it", () => {
    const t = ticks(-24, 18, 10);
    expect(t).toContain(0);
    // No -0, which would render as "-0" in a label.
    expect(t.every((v) => !Object.is(v, -0))).toBe(true);
  });

  it("does not drift on a fractional step", () => {
    // Repeated addition of 0.1 accumulates error; each step is re-rounded.
    expect(ticks(0, 0.5, 0.1)).toEqual([0, 0.1, 0.2, 0.3, 0.4, 0.5]);
  });

  it("returns nothing for a non-positive step rather than looping forever", () => {
    expect(ticks(0, 10, 0)).toEqual([]);
    expect(ticks(0, 10, -1)).toEqual([]);
  });
});

describe("histogramOutline", () => {
  const x = (bin: number) => bin * 10;
  const y = linearScale([0, 1], [0, 100], true);

  it("steps across each bin rather than joining bin centres", () => {
    // Two points per bin: a bin covers an interval, it is not a sample.
    const d = histogramOutline([1, 1], x, y, 2, 2).split(" ");
    expect(d).toEqual([
      "0.0,100.0",
      "0.0,50.0",
      "10.0,50.0",
      "10.0,50.0",
      "20.0,50.0",
      "20.0,100.0",
    ]);
  });

  it("closes to the baseline at both ends so the shape can be filled", () => {
    const d = histogramOutline([1], x, y, 1, 1).split(" ");
    expect(d[0]).toBe("0.0,100.0");
    expect(d[d.length - 1]).toBe("10.0,100.0");
  });

  it("draws a flat baseline for an empty frame instead of dividing by zero", () => {
    const d = histogramOutline([0, 0], x, y, 0, 2);
    expect(d).not.toContain("NaN");
  });

  it("stops at the visible range rather than running off the plot", () => {
    // The record holds 200 bins; a chart showing 110 must not draw the rest.
    const d = histogramOutline([1, 1, 1, 1], x, y, 4, 2);
    expect(d.split(" ").length).toBe(2 * 2 + 2);
  });
});

describe("populationRadius", () => {
  it("keeps a sparse band visible but small", () => {
    const tiny = populationRadius(0.0004, 3, 13);
    expect(tiny).toBeGreaterThanOrEqual(3);
    expect(tiny).toBeLessThan(populationRadius(0.5, 3, 13));
  });

  it("scales by area, not by radius", () => {
    // Four times the population is twice the radius above the floor, so the
    // eye compares areas rather than over-reading the big bands.
    const a = populationRadius(0.04, 0, 10);
    const b = populationRadius(0.16, 0, 10);
    expect(b / a).toBeCloseTo(2, 6);
  });
});

describe("plotArea and polyline", () => {
  it("insets by the margins", () => {
    expect(plotArea(500, 300, { left: 40, right: 20, top: 10, bottom: 30 })).toEqual({
      left: 40,
      right: 480,
      top: 10,
      bottom: 270,
    });
  });

  it("renders points at one decimal, which is sub-pixel enough and keeps SVG small", () => {
    expect(
      polyline([
        [1.23456, 2],
        [3, 4],
      ]),
    ).toBe("1.2,2.0 3.0,4.0");
  });
});

describe("niceStep", () => {
  it("keeps the gridline count readable across the whole range a frame can have", () => {
    // The bug this replaced: a fixed 2% step gave 51 labelled lines on a
    // near-black frame, across a plot about 230px tall.
    for (const hi of [0.01, 0.03, 0.12, 0.45, 1]) {
      const lines = Math.floor(hi / niceStep(hi)) + 1;
      expect(lines, `${hi}`).toBeGreaterThanOrEqual(3);
      expect(lines, `${hi}`).toBeLessThanOrEqual(11);
    }
  });

  it("only ever picks a 1, 2 or 5 times a power of ten", () => {
    for (const hi of [0.007, 0.03, 0.4, 3, 17, 240]) {
      const mantissa = niceStep(hi) / 10 ** Math.floor(Math.log10(niceStep(hi)));
      expect([1, 2, 5, 10]).toContain(Math.round(mantissa));
    }
  });

  it("never picks a step whose labels would repeat at whole-percent precision", () => {
    // The bug this caught: a 0.03 ceiling picks a 0.5% step, and rounding those
    // to whole percent printed "0%, 0%, 1%, 2%, 2%, 2%, 3%". The chart now
    // widens the format when the step is sub-percent; this pins the condition.
    for (const hi of [0.01, 0.03, 0.12, 0.45, 1]) {
      const s = niceStep(hi);
      const labels = Array.from({ length: Math.floor(hi / s) + 1 }, (_, i) =>
        (i * s * 100).toFixed(s < 0.01 ? 1 : 0),
      );
      expect(new Set(labels).size, `${hi}`).toBe(labels.length);
    }
  });

  it("does not divide by zero or loop on a degenerate range", () => {
    expect(niceStep(0)).toBe(1);
    expect(niceStep(-1)).toBe(1);
    expect(niceStep(1, 0)).toBe(1);
  });
});

describe("extent", () => {
  it("reports the smallest and largest value", () => {
    expect(extent([3, -7, 0, 12])).toEqual([-7, 12]);
  });

  it("has no extent to report for no values", () => {
    expect(extent([])).toEqual([0, 0]);
  });
});

describe("paddedBounds", () => {
  it("covers every value, rounded out to a multiple of the step", () => {
    const [lo, hi] = paddedBounds([-6.8, 14.2, 0.4]);
    expect(lo).toBeLessThanOrEqual(-6.8);
    expect(hi).toBeGreaterThanOrEqual(14.2);
    expect(Math.abs(lo % 2)).toBe(0);
    expect(Math.abs(hi % 2)).toBe(0);
  });

  it("always includes zero, so the chart is read against neutral", () => {
    const [lo, hi] = paddedBounds([4, 9, 11]);
    expect(lo).toBeLessThanOrEqual(0);
    expect(hi).toBeGreaterThan(11);
  });

  it("pads a nearly flat set rather than collapsing the axis", () => {
    expect(paddedBounds([0, 0, 0])).toEqual([-2, 2]);
  });

  // The padding is what makes a chart readable and what must never reach the
  // colour ramp: 19 is inside the ramp reference, and the axis it is drawn on is
  // not.
  it("can pad past a value that is itself inside the ramp reference", () => {
    expect(paddedBounds([19])[1]).toBeGreaterThan(19);
  });
});

describe("niceStep as a cast axis step", () => {
  // A fixed step of 10 left a mild frame — everything between -4 and +6 — with
  // no label but zero, on a chart whose whole rule is to read the value off the
  // axis rather than off the hue.
  it("labels a mild cast range more than once", () => {
    const [lo, hi] = paddedBounds([-4, 6]);
    expect(ticks(lo, hi, niceStep(hi - lo)).filter((t) => t !== 0).length).toBeGreaterThan(1);
  });

  it("still keeps a wide range to a handful of lines", () => {
    const [lo, hi] = paddedBounds([-40, 55]);
    expect(ticks(lo, hi, niceStep(hi - lo)).length).toBeLessThanOrEqual(9);
  });
});
