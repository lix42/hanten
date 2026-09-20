import { describe, expect, it } from "vite-plus/test";
import {
  addPatch,
  formatPatch,
  patchCount,
  patchedFrameCount,
  patchesFor,
  removePatch,
  type Patch,
  type Patches,
} from "./patches";

const patch = (id: string, label = id): Patch => ({
  id,
  label,
  x: 0.25,
  y: 0.5,
  width: 0.125,
  height: 0.0625,
});

describe("addPatch", () => {
  it("keeps the drawing order within a frame", () => {
    const one = addPatch({}, "a", patch("p1"));
    const two = addPatch(one, "a", patch("p2"));
    expect(patchesFor(two, "a").map((p) => p.id)).toEqual(["p1", "p2"]);
  });

  it("leaves other frames alone", () => {
    const patches = addPatch(addPatch({}, "a", patch("p1")), "b", patch("p2"));
    expect(patchesFor(patches, "a")).toHaveLength(1);
    expect(patchesFor(patches, "b")).toHaveLength(1);
  });
});

describe("removePatch", () => {
  it("removes one and keeps its siblings", () => {
    const patches = addPatch(addPatch({}, "a", patch("p1")), "a", patch("p2"));
    expect(patchesFor(removePatch(patches, "a", "p1"), "a").map((p) => p.id)).toEqual(["p2"]);
  });

  // So "how many frames have patches" stays a key count, the rule `setNote`
  // follows for a blanked note.
  it("drops the frame's key with its last patch", () => {
    const patches = addPatch({}, "a", patch("p1"));
    expect(removePatch(patches, "a", "p1")).toEqual({});
  });

  it("returns the same object when nothing matched", () => {
    const patches: Patches = addPatch({}, "a", patch("p1"));
    expect(removePatch(patches, "a", "nope")).toBe(patches);
    expect(removePatch(patches, "zzz", "p1")).toBe(patches);
  });

  // Two patches may carry the same label on nearly the same rectangle — "white
  // 1" and "white 2" on one shirt — so deletion has to go by id alone.
  it("deletes only the patch asked for, not its twin", () => {
    const twins = addPatch(addPatch({}, "a", patch("p1", "white")), "a", patch("p2", "white"));
    expect(patchesFor(removePatch(twins, "a", "p1"), "a").map((p) => p.id)).toEqual(["p2"]);
  });
});

describe("counting", () => {
  it("counts patches and the frames carrying them", () => {
    const patches = addPatch(
      addPatch(addPatch({}, "a", patch("p1")), "a", patch("p2")),
      "b",
      patch("p3"),
    );
    expect(patchCount(patches)).toBe(3);
    expect(patchedFrameCount(patches)).toBe(2);
    expect(patchCount({})).toBe(0);
    expect(patchedFrameCount({})).toBe(0);
  });
});

describe("formatPatch", () => {
  // Percentages of the frame, because the renditions of one frame need not
  // share a pixel size — a percentage is the one spelling true of all of them.
  it("states the label and the rectangle as percentages", () => {
    expect(formatPatch(patch("p1", "white shirt"))).toBe(
      '- "white shirt" — x 25.0% y 50.0% w 12.5% h 6.3%',
    );
  });

  // Quoted so a label containing a dash or a percent sign cannot be read as
  // part of the geometry.
  it("quotes the label", () => {
    expect(formatPatch({ ...patch("p1"), label: 'the "white" wall — 50%' })).toContain(
      '- "the \\"white\\" wall — 50%" —',
    );
  });
});
