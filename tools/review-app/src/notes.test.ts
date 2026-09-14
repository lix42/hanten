import { describe, expect, it } from "vite-plus/test";
import { formatNotes, noteCount, notesScope, setNote, type Notes } from "./notes";

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

describe("formatNotes", () => {
  it("lists noted frames in set order, whatever order they were written", () => {
    const notes = { c: "third note", a: "first note" };
    expect(formatNotes(frames, notes, "Ektar roll")).toBe(
      "# Review notes — Ektar roll\n\n" +
        "2 of 3 frames noted\n\n" +
        "## F1 — first\n\nfirst note\n\n" +
        "## F3 — third\n\nthird note\n",
    );
  });

  it("omits the heading's dash when the set has no title", () => {
    expect(formatNotes(frames, { a: "x" })).toContain("# Review notes\n");
  });

  it("trims each note, and skips whitespace-only ones", () => {
    expect(formatNotes(frames, { a: "  padded  ", b: "   " })).toContain("1 of 3 frames noted");
    expect(formatNotes(frames, { a: "  padded  " })).toContain("## F1 — first\n\npadded\n");
  });

  // Empty rather than a header with nothing under it: the copy button is
  // disabled on this, so it must be distinguishable from a real block.
  it("is empty when nothing is noted", () => {
    expect(formatNotes(frames, {})).toBe("");
    expect(formatNotes([], { a: "x" })).toBe("");
  });
});
