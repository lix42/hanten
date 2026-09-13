import { describe, expect, it } from "vite-plus/test";
import { domId } from "./domId";

describe("domId", () => {
  it("leaves ordinary ids readable", () => {
    expect(domId("E1", "chr-generic")).toBe("chart--E1--chr_2d_generic");
  });

  // The failure it exists for: `)` closes `url(#…)` early, the gradient
  // reference resolves to nothing, and the curves lose their stroke silently.
  it("encodes everything that would break a paint URL", () => {
    for (const hostile of ["a)b", 'a"b', "a b", "a#b", "a'b", "a(b"]) {
      expect(domId(hostile)).toMatch(/^[A-Za-z0-9_-]+$/);
    }
  });

  // Two charts sharing an id both resolve to the first gradient, which paints
  // one frame's curves with another's ramp — and colour is the encoding there.
  it("keeps ids that differ, different", () => {
    const ids = [
      domId("a-b", "c"),
      domId("a", "b-c"),
      domId("a)b"),
      domId("a-b"),
      domId("a", "b"),
      domId("ab"),
    ];
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("starts with a letter, whatever it was given", () => {
    expect(domId("1", "2")).toMatch(/^chart/);
  });
});
