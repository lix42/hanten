import { describe, expect, it } from "vite-plus/test";
import { assetId, contentTypeFor, createAssetMap, type StatMtime } from "./assets";

const AT = "/sets/tone/E1-shoulder.jpg";
const stat: StatMtime = (path) => (path === AT ? 1700 : undefined);

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
    const early = createAssetMap(() => 1000).register(AT);
    const later = createAssetMap(() => 2000).register(AT);
    expect(early).not.toBe(later);
    expect(later).toBe(`/img/${assetId(AT)}?v=2000`);
  });

  it("gives one file one id however many renditions name it", () => {
    let stats = 0;
    const assets = createAssetMap((path) => {
      stats += 1;
      return path === AT ? 1700 : undefined;
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
