import { describe, expect, it } from "vite-plus/test";
import { assetId, contentTypeFor, createAssetMap, thumbnailUrl, type StatFile } from "./assets";
import { PIPELINE_VERSION } from "./thumbs";

const AT = "/sets/tone/E1-shoulder.jpg";
const stat: StatFile = (path) => (path === AT ? { mtimeMs: 1700, size: 42 } : undefined);

describe("thumbnailUrl", () => {
  it("asks for a size of the same file, keeping the id and the version stamp", () => {
    const url = createAssetMap(stat).register(AT);
    // Not a route of its own: a thumbnail is another cache entry for this
    // version of this file, so a re-render still changes it along with the rest.
    // `&t=` is the generation pipeline's version: the response is `immutable`
    // for a year, so with the source unchanged a pipeline fix that did not move
    // the URL would never reach a browser that has already viewed the set.
    expect(thumbnailUrl(url, 208)).toBe(`${url}&w=208&t=${String(PIPELINE_VERSION)}`);
  });
});

describe("createAssetMap", () => {
  it("only serves files that were registered", () => {
    const assets = createAssetMap(stat);
    assets.register(AT);
    expect(assets.get(assetId(AT))?.path).toBe(AT);
    // The point of the map: an id nobody registered names nothing, so a path can
    // never be taken from the URL.
    expect(assets.get(assetId("/etc/passwd"))).toBeUndefined();
    expect(assets.get("../../../etc/passwd")).toBeUndefined();
  });

  it("versions the URL by mtime so a re-render is a different URL", () => {
    const early = createAssetMap(() => ({ mtimeMs: 1000, size: 42 })).register(AT);
    const later = createAssetMap(() => ({ mtimeMs: 2000, size: 42 })).register(AT);
    expect(early).not.toBe(later);
    expect(later).toBe(`/img/${assetId(AT)}?v=2000`);
  });

  it("gives one file one id however many renditions name it", () => {
    let stats = 0;
    const assets = createAssetMap((path) => {
      stats += 1;
      return path === AT ? { mtimeMs: 1700, size: 42 } : undefined;
    });
    // The common case: a rendition reused as its own preview.
    expect(assets.register(AT)).toBe(assets.register(AT));
    expect(stats).toBe(1);
    expect(assets.entries()).toHaveLength(1);
  });

  it("registers a file that does not exist rather than refusing the set", () => {
    // A config whose render failed is ordinary; the request 404s and the page
    // shows a gap, which beats refusing every comparison that did work.
    const assets = createAssetMap(stat);
    const url = assets.register("/sets/tone/never-rendered.jpg");
    expect(url).toMatch(/\?v=0$/);
    expect(assets.get(assetId("/sets/tone/never-rendered.jpg"))?.mtimeMs).toBe(0);
  });
});

describe("contentTypeFor", () => {
  it("types the formats a review set actually holds", () => {
    expect(contentTypeFor("/a/E1.jpg")).toBe("image/jpeg");
    expect(contentTypeFor("/a/E1.JPEG")).toBe("image/jpeg");
    expect(contentTypeFor("/a/E1.avif")).toBe("image/avif");
    expect(contentTypeFor("/a/E1.tif")).toBe("image/tiff");
  });

  it("does not guess at anything else", () => {
    // A stray file in a set directory must not come back as active content.
    expect(contentTypeFor("/a/review.json")).toBe("application/octet-stream");
    expect(contentTypeFor("/a/notes.html")).toBe("application/octet-stream");
    expect(contentTypeFor("/a/no-extension")).toBe("application/octet-stream");
  });
});
