import { describe, expect, it } from "vite-plus/test";
import { actionForKey, keyForConfigIndex, nextPointerMode, stepConfigIndex } from "./keys";

const NONE = {};

describe("actionForKey", () => {
  it("maps the number row left to right, with 0 as the tenth", () => {
    expect(actionForKey("1", NONE, 10)).toEqual({ kind: "config", index: 0 });
    expect(actionForKey("9", NONE, 10)).toEqual({ kind: "config", index: 8 });
    expect(actionForKey("0", NONE, 10)).toEqual({ kind: "config", index: 9 });
  });

  it("ignores a number with no config behind it", () => {
    expect(actionForKey("4", NONE, 2)).toBeNull();
    expect(actionForKey("0", NONE, 2)).toBeNull();
  });

  it("leaves modified keypresses to the browser and window manager", () => {
    // ⌘1 switches browser tabs; claiming it would break the user's own shortcut.
    expect(actionForKey("1", { meta: true }, 4)).toBeNull();
    expect(actionForKey("1", { ctrl: true }, 4)).toBeNull();
    expect(actionForKey("1", { alt: true }, 4)).toBeNull();
  });

  it("toggles zoom on f, in either case", () => {
    expect(actionForKey("f", NONE, 3)).toEqual({ kind: "zoom" });
    expect(actionForKey("F", NONE, 3)).toEqual({ kind: "zoom" });
  });

  it("toggles the charts on m, in either case", () => {
    expect(actionForKey("m", NONE, 3)).toEqual({ kind: "metrics" });
    expect(actionForKey("M", NONE, 3)).toEqual({ kind: "metrics" });
  });

  it("ignores everything else", () => {
    // `a`/`c`/`m`/`n` used to sit in this list and are now bound; `z` stands in
    // for a key that really is unbound.
    for (const key of ["z", "Enter", "ArrowLeft", " ", "Shift"]) {
      expect(actionForKey(key, NONE, 4)).toBeNull();
    }
  });
});

describe("keyForConfigIndex", () => {
  it("labels the buttons with the key that reaches them", () => {
    expect(keyForConfigIndex(0)).toBe("1");
    expect(keyForConfigIndex(8)).toBe("9");
    expect(keyForConfigIndex(9)).toBe("0");
  });

  it("has no key past the number row", () => {
    expect(keyForConfigIndex(10)).toBeUndefined();
  });
});

describe("stepConfigIndex", () => {
  it("steps forward and back", () => {
    expect(stepConfigIndex(1, 1, 4)).toBe(2);
    expect(stepConfigIndex(1, -1, 4)).toBe(0);
  });

  // Wrapping is the point: the configs are a handful of renderings of one frame,
  // cycled repeatedly, so running off one end and back on is the gesture.
  it("wraps at both ends", () => {
    expect(stepConfigIndex(3, 1, 4)).toBe(0);
    expect(stepConfigIndex(0, -1, 4)).toBe(3);
  });

  it("stays put when there is only one config", () => {
    expect(stepConfigIndex(0, 1, 1)).toBe(0);
    expect(stepConfigIndex(0, -1, 1)).toBe(0);
  });

  it("answers 0 rather than NaN for an empty set", () => {
    expect(stepConfigIndex(0, 1, 0)).toBe(0);
  });
});

describe("actionForKey — frame and config steps", () => {
  const step = (key: string) => actionForKey(key, {}, 6);

  it("maps j/k to frames and h/l to configs, in both cases", () => {
    expect(step("j")).toEqual({ kind: "frame", delta: 1 });
    expect(step("K")).toEqual({ kind: "frame", delta: -1 });
    expect(step("l")).toEqual({ kind: "configStep", delta: 1 });
    expect(step("H")).toEqual({ kind: "configStep", delta: -1 });
  });

  // A modified press belongs to the browser or the window manager.
  it("leaves modified presses alone", () => {
    expect(actionForKey("j", { meta: true }, 6)).toBeNull();
    expect(actionForKey("l", { ctrl: true }, 6)).toBeNull();
  });
});

describe("pointer modes", () => {
  it("maps p and i, in either case", () => {
    expect(actionForKey("p", NONE, 4)).toEqual({ kind: "pointerMode", mode: "patch" });
    expect(actionForKey("I", NONE, 4)).toEqual({ kind: "pointerMode", mode: "color" });
  });

  it("leaves them to the browser when modified", () => {
    expect(actionForKey("p", { meta: true }, 4)).toBeNull();
    expect(actionForKey("i", { ctrl: true }, 4)).toBeNull();
  });

  // Each key is its own toggle...
  it("turns a mode off when its own key is pressed again", () => {
    expect(nextPointerMode("patch", "patch")).toBe("off");
    expect(nextPointerMode("color", "color")).toBe("off");
  });

  // ...while the pair stays exclusive: `p` then `i` is colour, never both.
  it("switches straight between the two", () => {
    expect(nextPointerMode("patch", "color")).toBe("color");
    expect(nextPointerMode("off", "patch")).toBe("patch");
  });
});
