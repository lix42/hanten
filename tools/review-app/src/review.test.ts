import { describe, expect, it } from "vite-plus/test";
import { parseReview, type ResolveRendition } from "./review";

/**
 * Stands in for the server's resolver, which maps a rendition path to a file and
 * hands back a `/img/` URL. These tests are about parsing and validation, so it
 * only has to be deterministic and to report the path it was given.
 */
const RESOLVE: ResolveRendition = (path) => `resolved:${path}`;

function doc(overrides: Record<string, unknown> = {}) {
  return {
    schema_version: 1,
    configs: [
      { id: "shoulder", label: "shoulder" },
      { id: "none", label: "none" },
    ],
    images: [
      {
        id: "E1",
        label: "E1 — Ektar",
        renditions: { shoulder: "E1-shoulder.jpg", none: { src: "E1-none.jpg" } },
      },
    ],
    ...overrides,
  };
}

describe("parseReview", () => {
  it("puts every rendition path through the resolver", () => {
    const review = parseReview(doc(), RESOLVE);
    const image = review.images[0]!;
    expect(image.renditions["shoulder"]!.src).toBe("resolved:E1-shoulder.jpg");
    expect(image.renditions["none"]!.src).toBe("resolved:E1-none.jpg");
  });

  it("accepts a bare string as the rendition shorthand and mirrors it to preview", () => {
    const rendition = parseReview(doc(), RESOLVE).images[0]!.renditions["shoulder"]!;
    expect(rendition.preview).toBe(rendition.src);
  });

  it("keeps a distinct preview when one is given", () => {
    const review = parseReview(
      doc({
        images: [
          {
            id: "E1",
            renditions: { shoulder: { src: "big.jpg", preview: "thumb.jpg" } },
          },
        ],
      }),
      RESOLVE,
    );
    const rendition = review.images[0]!.renditions["shoulder"]!;
    expect(rendition.src).toBe("resolved:big.jpg");
    expect(rendition.preview).toBe("resolved:thumb.jpg");
  });

  it("falls back to the id when an image states no label", () => {
    const review = parseReview(
      doc({ images: [{ id: "P4", renditions: { none: "p4.jpg" } }] }),
      RESOLVE,
    );
    expect(review.images[0]!.label).toBe("P4");
  });

  it("allows an image to be missing a rendition", () => {
    const review = parseReview(
      doc({ images: [{ id: "E1", renditions: { shoulder: "a.jpg" } }] }),
      RESOLVE,
    );
    // The comparison is still worth showing; the gap is rendered, not fatal.
    expect(review.images[0]!.renditions["none"]).toBeUndefined();
  });

  it("rejects a rendition keyed by an undeclared config, naming the typo", () => {
    expect(() =>
      parseReview(doc({ images: [{ id: "E1", renditions: { shouldre: "a.jpg" } }] }), RESOLVE),
    ).toThrow(/shouldre.*not one of the declared configs \(shoulder, none\)/s);
  });

  it("rejects a rendition that states only one dimension", () => {
    // Half a size reserves nothing, and it would also slip past the load-time
    // check that compares declared against natural dimensions.
    for (const partial of [{ width: 100 }, { height: 100 }]) {
      expect(() =>
        parseReview(
          doc({
            images: [{ id: "E1", renditions: { none: { src: "a.jpg", ...partial } } }],
          }),
          RESOLVE,
        ),
      ).toThrow(/without the other; give both or neither/);
    }
    // Both, or neither, stay valid.
    expect(() =>
      parseReview(
        doc({
          images: [
            {
              id: "E1",
              renditions: { none: { src: "a.jpg", width: 100, height: 50 } },
            },
          ],
        }),
        RESOLVE,
      ),
    ).not.toThrow();
  });

  it("rejects a schema version it cannot read", () => {
    expect(() => parseReview(doc({ schema_version: 2 }), RESOLVE)).toThrow(
      /schema_version must be 1/,
    );
  });

  it("rejects duplicate config ids", () => {
    expect(() =>
      parseReview(
        doc({
          configs: [
            { id: "a", label: "A" },
            { id: "a", label: "A again" },
          ],
        }),
        RESOLVE,
      ),
    ).toThrow(/two entries with id "a"/);
  });

  it("rejects an empty config list", () => {
    expect(() => parseReview(doc({ configs: [] }), RESOLVE)).toThrow(/at least one/);
  });

  it("names the failing path when a field has the wrong type", () => {
    expect(() =>
      parseReview(doc({ images: [{ id: "E1", renditions: { none: 42 } }] }), RESOLVE),
    ).toThrow(/images\[0\]\.renditions\.none must be an object, got number/);
  });
});
