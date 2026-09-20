import { describe, expect, it } from "vite-plus/test";
import { formatReview, noteCount, notesScope, setNote, type Notes } from "./notes";
import type { Patch, Patches } from "./patches";

const frames = [
  { id: "a", label: "F1 — first" },
  { id: "b", label: "F2 — second" },
  { id: "c", label: "F3 — third" },
];

describe("setNote", () => {
  it("adds and replaces a note", () => {
    const one = setNote({}, "a", "clips");
    expect(one).toEqual({ a: "clips" });
    expect(setNote(one, "a", "clips badly")).toEqual({ a: "clips badly" });
  });

  // Blank is the same gesture as never having written one, so the key goes.
  it("drops the note when the text is blank", () => {
    expect(setNote({ a: "clips" }, "a", "")).toEqual({});
    expect(setNote({ a: "clips" }, "a", "   \n ")).toEqual({});
  });

  it("keeps the raw text, trimming only to judge blankness", () => {
    expect(setNote({}, "a", "  two words  ")).toEqual({ a: "  two words  " });
  });

  // A signal set from an unchanged object must not re-render every dialog.
  it("returns the same object when nothing changed", () => {
    const notes: Notes = { a: "clips" };
    expect(setNote(notes, "a", "clips")).toBe(notes);
    expect(setNote(notes, "b", "")).toBe(notes);
  });

  it("leaves other frames alone", () => {
    expect(setNote({ a: "one", b: "two" }, "a", "")).toEqual({ b: "two" });
  });
});

describe("noteCount", () => {
  it("counts frames with notes", () => {
    expect(noteCount({})).toBe(0);
    expect(noteCount({ a: "x", b: "y" })).toBe(2);
  });
});

describe("notesScope", () => {
  it("changes when the set or its frame list changes", () => {
    const base = notesScope("/s/review.json", ["a", "b"]);
    expect(notesScope("/s/review.json", ["a", "b"])).toBe(base);
    expect(notesScope("/other/review.json", ["a", "b"])).not.toBe(base);
    expect(notesScope("/s/review.json", ["a", "b", "c"])).not.toBe(base);
    expect(notesScope("/s/review.json", ["b", "a"])).not.toBe(base);
  });
});

const NO_PATCHES: Patches = {};

const patch = (id: string, label: string, x: number, y: number): Patch => ({
  id,
  label,
  x,
  y,
  width: 0.2,
  height: 0.1,
});

describe("formatReview", () => {
  it("lists noted frames in set order, whatever order they were written", () => {
    const notes = { c: "third note", a: "first note" };
    expect(formatReview(frames, notes, NO_PATCHES, "Ektar roll")).toBe(
      "# Review notes — Ektar roll\n\n" +
        "2 of 3 frames noted\n\n" +
        "## F1 — first\n\nfirst note\n\n" +
        "## F3 — third\n\nthird note\n",
    );
  });

  it("omits the heading's dash when the set has no title", () => {
    expect(formatReview(frames, { a: "x" }, NO_PATCHES)).toContain("# Review notes\n");
  });

  it("trims each note, and skips whitespace-only ones", () => {
    expect(formatReview(frames, { a: "  padded  ", b: "   " }, NO_PATCHES)).toContain(
      "1 of 3 frames noted",
    );
    expect(formatReview(frames, { a: "  padded  " }, NO_PATCHES)).toContain(
      "## F1 — first\n\npadded\n",
    );
  });

  // Empty rather than a header with nothing under it: the copy button is
  // disabled on this, so it must be distinguishable from a real block.
  it("is empty when nothing is noted", () => {
    expect(formatReview(frames, {}, NO_PATCHES)).toBe("");
    expect(formatReview([], { a: "x" }, NO_PATCHES)).toBe("");
  });
});

describe("formatReview with patches", () => {
  const patches: Patches = { b: [patch("p1", "cloud", 0.1, 0.2)] };

  // A patch is as much a review finding as a sentence is, so a frame carrying
  // only patches has to appear — listing it by note count alone would drop it.
  it("lists a frame that has patches but no note", () => {
    const block = formatReview(frames, {}, patches, "Ektar roll");
    expect(block).toContain("0 of 3 frames noted · 1 patch on 1 frame");
    expect(block).toContain(
      '## F2 — second\n\nPatches:\n- "cloud" — x 10.0% y 20.0% w 20.0% h 10.0%\n',
    );
  });

  it("puts a frame's note above its patches", () => {
    expect(formatReview(frames, { b: "soft" }, patches)).toContain(
      '## F2 — second\n\nsoft\n\nPatches:\n- "cloud"',
    );
  });

  it("counts patches and the frames they sit on", () => {
    const many: Patches = {
      a: [patch("p1", "sky", 0, 0), patch("p2", "skin", 0.5, 0.5)],
      b: [patch("p3", "cloud", 0.1, 0.2)],
    };
    expect(formatReview(frames, {}, many)).toContain("3 patches on 2 frames");
  });

  it("says nothing about patches when there are none", () => {
    expect(formatReview(frames, { a: "x" }, NO_PATCHES)).toContain("1 of 3 frames noted\n");
    expect(formatReview(frames, { a: "x" }, NO_PATCHES)).not.toContain("patch");
  });
});
