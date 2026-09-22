import { describe, expect, it } from "vite-plus/test";
import { SYNTHETIC_METRICS } from "./charts/fixture";
import { parseMetrics } from "./charts/metrics";
import { parseReview, type LoadMetrics, type ResolveRendition } from "./review";

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

  it("gives the bare-string shorthand a preview as well as a src", () => {
    // Both roles are resolved, from the one path the shorthand states — what
    // each resolves *to* is the resolver's business, and the production one
    // answers the preview role with a size. `RESOLVE` ignores the role, so the
    // two come back equal here; the next test is what pins the roles.
    const rendition = parseReview(doc(), RESOLVE).images[0]!.renditions["shoulder"]!;
    expect(rendition.src).toBe("resolved:E1-shoulder.jpg");
    expect(rendition.preview).toBe("resolved:E1-shoulder.jpg");
  });

  it("tells the resolver which role each URL is for", () => {
    // The strip shows the same file in a 104px box, so the server answers the
    // preview role with a downscaled URL. Both roles resolve the *same path*,
    // including the shorthand form, or a set whose renditions are full-size
    // scans would hand the strip a 7 MB file again.
    const roles: ResolveRendition = (path, _at, role) => `${role}:${path}`;
    const renditions = parseReview(doc(), roles).images[0]!.renditions;
    expect(renditions["shoulder"]).toMatchObject({
      src: "src:E1-shoulder.jpg",
      preview: "preview:E1-shoulder.jpg",
    });
    expect(renditions["none"]).toMatchObject({
      src: "src:E1-none.jpg",
      preview: "preview:E1-none.jpg",
    });
  });

  it("resolves a declared preview in the preview role too", () => {
    // One rule rather than two: a set that names a thumbnail has no idea how big
    // the strip's box is, and one that points `preview` at a second full-size
    // file would otherwise be the one case left unprotected.
    const roles: ResolveRendition = (path, _at, role) => `${role}:${path}`;
    const review = parseReview(
      doc({
        images: [{ id: "E1", renditions: { shoulder: { src: "big.jpg", preview: "thumb.jpg" } } }],
      }),
      roles,
    );
    expect(review.images[0]!.renditions["shoulder"]!.preview).toBe("preview:thumb.jpg");
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

  it("does not let an inherited Object member pass as a rendition", () => {
    // The record replaced a `Map`, and a plain object answers `toString`,
    // `constructor` and friends from its prototype. A config id spelling one of
    // those, with no rendition for an image, would read as *present* and render
    // a broken image where the missing-rendition gap belongs.
    const review = parseReview(
      doc({
        configs: [
          { id: "toString", label: "toString" },
          { id: "real", label: "real" },
        ],
        images: [{ id: "E1", renditions: { real: "a.jpg" } }],
      }),
      RESOLVE,
    );
    const renditions = review.images[0]!.renditions;
    expect(renditions["real"]?.src).toBe("resolved:a.jpg");
    expect(renditions["toString"]).toBeUndefined();
    expect(renditions["constructor"]).toBeUndefined();
    expect(renditions["hasOwnProperty"]).toBeUndefined();
  });

  it("names the failing path when a field has the wrong type", () => {
    expect(() =>
      parseReview(doc({ images: [{ id: "E1", renditions: { none: 42 } }] }), RESOLVE),
    ).toThrow(/images\[0\]\.renditions\.none must be an object, got number/);
  });
});

/**
 * Stands in for the server's record reader, which resolves the path against the
 * review file's directory and parses what it finds. Here it answers from one
 * fixture, and reports what it was asked for.
 */
const LOAD_METRICS: LoadMetrics = (path) =>
  path === "missing.json"
    ? { metricsError: `cannot read ${path}` }
    : { metrics: parseMetrics(SYNTHETIC_METRICS) };

function withMetrics(metrics: unknown) {
  return doc({
    images: [
      {
        id: "E1",
        renditions: { shoulder: { src: "E1-shoulder.jpg", metrics }, none: "E1-none.jpg" },
      },
    ],
  });
}

describe("parseReview and duplicate image ids", () => {
  // The charts address their SVG gradients by an id built from the image and the
  // config, and two identical ids in one document both resolve to the first —
  // so the second frame's cast curves would take the first frame's ramp.
  it("refuses two images with the same id", () => {
    const duplicated = doc({
      images: [
        { id: "E1", renditions: { shoulder: "a.jpg" } },
        { id: "E1", renditions: { none: "b.jpg" } },
      ],
    });
    expect(() => parseReview(duplicated, RESOLVE)).toThrow(/two entries with id "E1"/);
  });
});

describe("parseReview and measurements", () => {
  it("attaches the record a rendition names", () => {
    const image = parseReview(withMetrics("E1-shoulder.metrics.json"), RESOLVE, LOAD_METRICS)
      .images[0]!;
    expect(image.renditions["shoulder"]!.metrics?.schemaVersion).toBe(2);
    expect(image.renditions["shoulder"]!.metricsError).toBeUndefined();
  });

  // The app must not require the data it did not have yesterday: a set written
  // before measurements existed, or rendered with --no-metrics, still loads.
  it("leaves a rendition naming no record unmeasured", () => {
    const image = parseReview(withMetrics(undefined), RESOLVE, LOAD_METRICS).images[0]!;
    expect(image.renditions["shoulder"]!.metrics).toBeUndefined();
    expect(image.renditions["none"]!.metrics).toBeUndefined();
  });

  // A record that cannot be read costs its own charts and nothing else. The
  // picture is what this app exists to show, and the other four configs of the
  // comparison are still good.
  it("reports an unreadable record without refusing the set", () => {
    const image = parseReview(withMetrics("missing.json"), RESOLVE, LOAD_METRICS).images[0]!;
    expect(image.renditions["shoulder"]!.metricsError).toContain("cannot read missing.json");
    expect(image.renditions["shoulder"]!.metrics).toBeUndefined();
    expect(image.renditions["none"]!.src).toBe("resolved:E1-none.jpg");
  });

  it("still refuses a malformed metrics path, which is a typo in the document", () => {
    expect(() => parseReview(withMetrics(7), RESOLVE, LOAD_METRICS)).toThrow(
      /renditions\.shoulder\.metrics must be a non-empty string/,
    );
  });

  // Every test that is not about measurements parses without a reader.
  it("ignores a named record when nothing can read one", () => {
    const image = parseReview(withMetrics("E1-shoulder.metrics.json"), RESOLVE).images[0]!;
    expect(image.renditions["shoulder"]!.metrics).toBeUndefined();
    expect(image.renditions["shoulder"]!.metricsError).toBeUndefined();
  });
});

describe("parseReview — producer", () => {
  const HANTEN = {
    kind: "hanten",
    label: "candidate",
    nc_version: "0.1.0",
    git_commit: "2664a0ddbdd5",
    git_dirty: true,
    pipeline_version: 5,
    target: "aarch64-apple-darwin",
  };

  function withProducer(producer: unknown) {
    return doc({
      configs: [
        { id: "shoulder", label: "shoulder", producer },
        { id: "none", label: "none" },
      ],
    });
  }

  it("reads a build's identity into the model", () => {
    const [build, plain] = parseReview(withProducer(HANTEN), RESOLVE).configs;
    expect(build!.producer).toEqual({
      kind: "hanten",
      label: "candidate",
      ncVersion: "0.1.0",
      gitCommit: "2664a0ddbdd5",
      gitDirty: true,
      pipelineVersion: 5,
      target: "aarch64-apple-darwin",
    });
    // Optional: every set written before a build axis existed still loads.
    expect(plain!.producer).toBeUndefined();
  });

  it("reads a build that identifies itself only by name", () => {
    const configs = parseReview(withProducer({ kind: "hanten", label: "only" }), RESOLVE).configs;
    expect(configs[0]!.producer).toEqual({
      kind: "hanten",
      label: "only",
      ncVersion: undefined,
      gitCommit: undefined,
      gitDirty: undefined,
      pipelineVersion: undefined,
      target: undefined,
    });
  });

  // `analysis/review-reference-cells` adds a variant here rather than a second
  // block: both answer "which cell did nc-as-configured not render?".
  it("reads an outside producer", () => {
    const configs = parseReview(
      withProducer({ kind: "external", label: "NLP", note: "nlp/2026-09-09" }),
      RESOLVE,
    ).configs;
    expect(configs[0]!.producer).toEqual({
      kind: "external",
      label: "NLP",
      note: "nlp/2026-09-09",
    });
  });

  // Unlike an unreadable metric record, wrong provenance is the exact lie a build
  // comparison exists to rule out — so it refuses the set rather than costing only
  // itself.
  it("refuses a kind it does not know", () => {
    expect(() => parseReview(withProducer({ kind: "nc", label: "old" }), RESOLVE)).toThrow(
      /configs\[0\]\.producer\.kind must be "hanten" or "external", got "nc"/,
    );
  });

  it("refuses a producer with no label", () => {
    expect(() => parseReview(withProducer({ kind: "hanten" }), RESOLVE)).toThrow(
      /configs\[0\]\.producer\.label must be a non-empty string/,
    );
  });

  it("refuses an identity field of the wrong type", () => {
    expect(() => parseReview(withProducer({ ...HANTEN, git_dirty: "yes" }), RESOLVE)).toThrow(
      /configs\[0\]\.producer\.git_dirty must be a boolean/,
    );
    expect(() => parseReview(withProducer({ ...HANTEN, git_commit: 7 }), RESOLVE)).toThrow(
      /configs\[0\]\.producer\.git_commit must be a non-empty string/,
    );
  });
});
