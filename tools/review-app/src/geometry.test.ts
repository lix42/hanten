import { describe, expect, it } from "vite-plus/test";
import {
  MIN_PATCH_PX,
  containedBox,
  isDragWorthKeeping,
  normalizePoint,
  paintedBox,
  placeReadout,
  rectFromDrag,
  rectToBox,
  samplePlan,
} from "./geometry";

const pane = { x: 0, y: 0, width: 400, height: 200 };

describe("containedBox", () => {
  // The case that makes this necessary: a portrait frame in a landscape pane is
  // letterboxed across most of the width, so a patch measured against the
  // element box would sit nowhere near the subject.
  it("letterboxes a portrait image sideways", () => {
    expect(containedBox(pane, 100, 200)).toEqual({ x: 150, y: 0, width: 100, height: 200 });
  });

  it("letterboxes a wide image vertically", () => {
    expect(containedBox(pane, 800, 200)).toEqual({ x: 0, y: 50, width: 400, height: 100 });
  });

  it("fills the box exactly when the aspects match", () => {
    expect(containedBox(pane, 800, 400)).toEqual(pane);
  });

  // An image that has not decoded reports 0x0; dividing by it would place the
  // overlay at NaN, which CSS drops silently.
  it("leaves the box alone when the natural size is unknown", () => {
    expect(containedBox(pane, 0, 0)).toEqual(pane);
  });
});

describe("paintedBox", () => {
  it("contains in fit and takes the element box in fullsize", () => {
    expect(paintedBox(pane, 100, 200, "fit")).toEqual(containedBox(pane, 100, 200));
    expect(paintedBox(pane, 100, 200, "fullsize")).toEqual(pane);
  });
});

describe("normalizePoint", () => {
  it("reports a fraction of the painted picture, not of the pane", () => {
    const painted = { x: 100, y: 0, width: 200, height: 200 };
    expect(normalizePoint({ x: 200, y: 50 }, painted)).toEqual({ x: 0.5, y: 0.25 });
  });

  it("clamps to the picture", () => {
    expect(normalizePoint({ x: -50, y: 900 }, pane)).toEqual({ x: 0, y: 1 });
  });
});

describe("rectFromDrag", () => {
  // There is no wrong direction to select a patch in.
  it("normalises either drag direction to the same rectangle", () => {
    const a = { x: 100, y: 50 };
    const b = { x: 300, y: 150 };
    const forward = rectFromDrag(a, b, pane);
    expect(forward).toEqual({ x: 0.25, y: 0.25, width: 0.5, height: 0.5 });
    expect(rectFromDrag(b, a, pane)).toEqual(forward);
  });

  it("survives a round trip back to pixels", () => {
    const rect = rectFromDrag({ x: 100, y: 50 }, { x: 300, y: 150 }, pane);
    expect(rectToBox(rect, pane)).toEqual({ x: 100, y: 50, width: 200, height: 100 });
  });

  // The payoff of storing against the image: the same patch on a picture drawn
  // at a different size lands on the same subject.
  it("maps onto a differently sized picture unchanged", () => {
    const rect = rectFromDrag({ x: 100, y: 50 }, { x: 300, y: 150 }, pane);
    expect(rectToBox(rect, { x: 0, y: 0, width: 800, height: 400 })).toEqual({
      x: 200,
      y: 100,
      width: 400,
      height: 200,
    });
  });
});

describe("isDragWorthKeeping", () => {
  // A click in patch mode is a miss, not a request for an empty rectangle.
  it("refuses a click and a hairline drag", () => {
    expect(isDragWorthKeeping({ x: 10, y: 10 }, { x: 10, y: 10 }, pane)).toBe(false);
    expect(isDragWorthKeeping({ x: 10, y: 10 }, { x: 40, y: 12 }, pane)).toBe(false);
  });

  it("accepts a drag in any direction once both axes clear the floor", () => {
    const far = MIN_PATCH_PX;
    expect(isDragWorthKeeping({ x: 10, y: 10 }, { x: 10 + far, y: 10 + far }, pane)).toBe(true);
    expect(isDragWorthKeeping({ x: 10, y: 10 }, { x: 10 - far, y: 10 - far }, pane)).toBe(true);
  });

  /*
    The floor is measured on the rectangle that would be *kept*, not on the raw
    drag. `rectFromDrag` clamps both corners into the picture, so a drag from an
    edge running outward covers plenty of screen and collapses to a sliver — and
    at the edge itself, to nothing. Judged on the raw endpoints these passed, and
    the patch drew as a line and copied as `w 0.0%`.
  */
  it("refuses a drag that clamps to a sliver against the picture's edge", () => {
    expect(isDragWorthKeeping({ x: 0, y: 50 }, { x: -40, y: 90 }, pane)).toBe(false);
    expect(isDragWorthKeeping({ x: 2, y: 50 }, { x: -40, y: 90 }, pane)).toBe(false);
    expect(isDragWorthKeeping({ x: 50, y: 200 }, { x: 90, y: 260 }, pane)).toBe(false);
  });

  it("still accepts a drag that runs off the edge but keeps enough inside", () => {
    expect(isDragWorthKeeping({ x: 30, y: 50 }, { x: -40, y: 90 }, pane)).toBe(true);
  });
});

describe("samplePlan", () => {
  // In `fit` one screen pixel covers several image pixels, and reading one of
  // them reports grain rather than the colour on screen.
  it("spans the image pixels one screen pixel covers", () => {
    const painted = { x: 0, y: 0, width: 400, height: 200 };
    // Centred on the point, not starting at it: a crop anchored at the mapped
    // position sits entirely below and right of the cursor, and across a sharp
    // edge reports the colour on the other side of it.
    expect(samplePlan({ x: 200, y: 100 }, painted, 4000, 2000, 1)).toEqual({
      x: 1995,
      y: 995,
      span: 10,
    });
  });

  // At span 1 the centring offset must be exactly zero: there is nothing to
  // average, and half a pixel of shift would report the neighbour instead.
  it("reduces to the pixel under the point when nothing is averaged", () => {
    const painted = { x: 0, y: 0, width: 400, height: 200 };
    expect(samplePlan({ x: 10, y: 20 }, painted, 400, 200, 1)).toEqual({ x: 10, y: 20, span: 1 });
    expect(samplePlan({ x: 137, y: 61 }, painted, 400, 200, 1)).toEqual({ x: 137, y: 61, span: 1 });
  });

  it("reads a single pixel at natural size", () => {
    expect(samplePlan({ x: 10, y: 20 }, pane, 400, 200, 1)?.span).toBe(1);
    expect(samplePlan({ x: 10, y: 20 }, pane, 400, 200, 1)).toMatchObject({ x: 10, y: 20 });
  });

  // A point on the far edge must still read pixels that exist.
  it("keeps the whole span inside the image at the edges", () => {
    const plan = samplePlan({ x: 400, y: 200 }, pane, 4000, 2000, 1);
    expect(plan).toBeDefined();
    expect(plan!.x + plan!.span).toBeLessThanOrEqual(4000);
    expect(plan!.y + plan!.span).toBeLessThanOrEqual(2000);
  });

  it("declines outside the picture and before it has decoded", () => {
    expect(samplePlan({ x: -1, y: 10 }, pane, 400, 200, 1)).toBeUndefined();
    expect(samplePlan({ x: 10, y: 201 }, pane, 400, 200, 1)).toBeUndefined();
    expect(samplePlan({ x: 10, y: 10 }, pane, 0, 0, 1)).toBeUndefined();
  });
});

describe("samplePlan on a HiDPI display", () => {
  // A raster image is rasterised at *device* resolution, so what the screen
  // shows is `width * ratio` samples across. Dividing by CSS pixels alone
  // averages a box twice as wide as the one the browser drew — a 4x error in
  // area on a 2x display.
  it("halves the span at devicePixelRatio 2", () => {
    const painted = { x: 0, y: 0, width: 400, height: 200 };
    expect(samplePlan({ x: 200, y: 100 }, painted, 4000, 2000, 1)?.span).toBe(10);
    expect(samplePlan({ x: 200, y: 100 }, painted, 4000, 2000, 2)?.span).toBe(5);
    expect(samplePlan({ x: 200, y: 100 }, painted, 4000, 2000, 3)?.span).toBe(3);
  });

  // `fullsize` on a 2x display draws two device pixels per image pixel, so
  // there is nothing left to average: one image pixel is the finest answer.
  it("never falls below one image pixel", () => {
    expect(samplePlan({ x: 10, y: 20 }, pane, 400, 200, 2)?.span).toBe(1);
    expect(samplePlan({ x: 10, y: 20 }, pane, 400, 200, 4)?.span).toBe(1);
  });

  // A ratio the browser could not sensibly report must not produce a zero or
  // negative span, which would make `drawImage` throw.
  it("falls back to 1 for a nonsensical ratio", () => {
    const painted = { x: 0, y: 0, width: 400, height: 200 };
    expect(samplePlan({ x: 200, y: 100 }, painted, 4000, 2000, 0)?.span).toBe(10);
    expect(samplePlan({ x: 200, y: 100 }, painted, 4000, 2000, -2)?.span).toBe(10);
  });
});

describe("placeReadout", () => {
  const chip = { width: 200, height: 100 };

  it("sits below and to the right of the cursor", () => {
    expect(placeReadout({ x: 10, y: 10 }, { width: 800, height: 600 }, chip, 16)).toEqual({
      left: 26,
      top: 26,
    });
  });

  // Flipped rather than slid back along the pane: sliding would put the chip
  // over the very pixel it is describing.
  it("flips across the cursor at the far edges", () => {
    expect(placeReadout({ x: 790, y: 590 }, { width: 800, height: 600 }, chip, 16)).toEqual({
      left: 574,
      top: 474,
    });
  });

  it("never places the chip off the near edges either", () => {
    const place = placeReadout({ x: 5, y: 5 }, { width: 100, height: 80 }, chip, 16);
    expect(place.left).toBe(0);
    expect(place.top).toBe(0);
  });
});
