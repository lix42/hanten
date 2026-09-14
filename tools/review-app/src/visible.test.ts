import { describe, expect, it } from "vite-plus/test";
import { OVERSCAN, currentFrame, frameForKey, mountWindow, sameIds } from "./visible";

const ids = ["a", "b", "c", "d", "e"];
const window_ = (visible: string[], overscan = 1) => [
  ...mountWindow(ids, new Set(visible), overscan),
];

describe("mountWindow", () => {
  it("mounts the visible frame and its neighbours", () => {
    expect(window_(["c"])).toEqual(["b", "c", "d"]);
  });

  it("spans every visible frame when two straddle the fold", () => {
    expect(window_(["b", "c"])).toEqual(["a", "b", "c", "d"]);
  });

  it("clamps at the head and the tail rather than wrapping", () => {
    expect(window_(["a"])).toEqual(["a", "b"]);
    expect(window_(["e"])).toEqual(["d", "e"]);
  });

  it("mounts only the visible frames at zero overscan", () => {
    expect(window_(["c"], 0)).toEqual(["c"]);
  });

  it("widens with overscan, and saturates at the whole list", () => {
    expect(window_(["c"], 2)).toEqual(ids);
    expect(window_(["c"], 99)).toEqual(ids);
  });

  // Before the first observer callback nothing is visible. Mounting nothing
  // would paint a page of blank shells on a set that has not been scrolled.
  it("mounts the head of the list when nothing is visible", () => {
    expect(window_([])).toEqual(["a", "b"]);
  });

  // A live edit to `review.json` can remove an image between the observer
  // reporting it and this running. A stale id must not empty the window.
  it("ignores visible ids that are no longer in the set", () => {
    expect(window_(["gone"])).toEqual(["a", "b"]);
    expect(window_(["gone", "d"])).toEqual(["c", "d", "e"]);
  });

  it("returns nothing for a set with no images", () => {
    expect([...mountWindow([], new Set(["a"]))]).toEqual([]);
  });

  it("defaults to the shipped overscan", () => {
    expect([...mountWindow(ids, new Set(["c"]))]).toEqual(window_(["c"], OVERSCAN));
  });
});

describe("sameIds", () => {
  it("is true for equal sets regardless of insertion order", () => {
    expect(sameIds(new Set(["a", "b"]), new Set(["b", "a"]))).toBe(true);
  });

  it("is false when membership differs at the same size", () => {
    expect(sameIds(new Set(["a", "b"]), new Set(["a", "c"]))).toBe(false);
  });

  it("is false when the sizes differ", () => {
    expect(sameIds(new Set(["a"]), new Set(["a", "b"]))).toBe(false);
  });

  it("is true for two empty sets", () => {
    expect(sameIds(new Set(), new Set())).toBe(true);
  });
});

describe("currentFrame", () => {
  const on = (...v: string[]) => new Set(v);

  it("is the lowest frame whose picture is on screen", () => {
    expect(currentFrame(ids, on("b", "c"), undefined)).toBe("c");
  });

  it("is that frame even when a higher one is more fully shown", () => {
    expect(currentFrame(ids, on("a", "e"), undefined)).toBe("e");
  });

  // Between frames the charts of one and the head of the next can fill the
  // screen with no picture at all; blanking the mark there would flicker it.
  it("keeps the previous answer when no picture is on screen", () => {
    expect(currentFrame(ids, on(), "d")).toBe("d");
  });

  it("falls back to the first frame when the previous one is gone", () => {
    expect(currentFrame(ids, on(), "removed")).toBe("a");
    expect(currentFrame(ids, on(), undefined)).toBe("a");
  });

  it("has no answer for a set with no frames", () => {
    expect(currentFrame([], on("a"), "a")).toBeUndefined();
  });
});

describe("frameForKey", () => {
  it("aligns the current frame first, in either direction", () => {
    expect(frameForKey(ids, "c", 1, false)).toBe("c");
    expect(frameForKey(ids, "c", -1, false)).toBe("c");
  });

  it("steps once the current frame is aligned", () => {
    expect(frameForKey(ids, "c", 1, true)).toBe("d");
    expect(frameForKey(ids, "c", -1, true)).toBe("b");
  });

  // No wrapping: `j` held down at the end must not jump back to the start.
  it("stays put past either end", () => {
    expect(frameForKey(ids, "e", 1, true)).toBeUndefined();
    expect(frameForKey(ids, "a", -1, true)).toBeUndefined();
  });

  it("starts at the first frame when nothing is current", () => {
    expect(frameForKey(ids, undefined, 1, false)).toBe("a");
    expect(frameForKey(ids, "removed", -1, true)).toBe("a");
  });

  it("does nothing for a set with no frames", () => {
    expect(frameForKey([], "a", 1, true)).toBeUndefined();
  });
});
