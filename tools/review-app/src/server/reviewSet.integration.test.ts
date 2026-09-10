import { describe, expect, it } from "vite-plus/test";
import { assetId } from "./assets";
import { loadReviewSet, resolveSetPath } from "./reviewSet";

/**
 * Reads the committed example off disk, so the parts the unit tests cannot
 * reach — the file read, resolution against the review file's own directory,
 * and the real `stat` behind every `/img/` URL — are exercised by something.
 * Nothing else in the suite touches the filesystem.
 */
describe("loadReviewSet against the bundled example", () => {
  it("parses it and gives every rendition a versioned /img/ URL", async () => {
    const set = await loadReviewSet(resolveSetPath({}, process.cwd()));

    expect(set.source).toBe("bundled-example");
    expect(set.path).toMatch(/public\/examples\/synthetic\/review\.json$/);
    expect(set.review.configs.length).toBeGreaterThan(0);
    expect(set.review.images.length).toBeGreaterThan(0);

    for (const image of set.review.images) {
      for (const config of set.review.configs) {
        const rendition = image.renditions[config.id];
        if (!rendition) continue;
        // A real file, so a real mtime: `?v=0` would mean the path resolved to
        // somewhere that does not exist.
        expect(rendition.src).toMatch(/^\/img\/[0-9a-f]{16}\?v=[1-9][0-9]*(\.[0-9]+)?$/);
      }
    }
  });

  it("resolves renditions against the review file, and registers each one", async () => {
    const set = await loadReviewSet(resolveSetPath({}, process.cwd()));
    const registered = set.assets.entries();
    expect(registered.length).toBeGreaterThan(0);

    for (const asset of registered) {
      // Resolution is relative to the review file's directory, not the cwd.
      expect(asset.path.startsWith(set.dir)).toBe(true);
      expect(asset.mtimeMs).toBeGreaterThan(0);
      // Every registered file is reachable by the id its URL carries.
      expect(set.assets.get(assetId(asset.path))?.path).toBe(asset.path);
    }
  });

  it("refuses a set that is not there, naming the path and the remedy", async () => {
    await expect(
      loadReviewSet({ path: "/no/such/set/review.json", source: "env" }),
    ).rejects.toThrow(/\/no\/such\/set\/review\.json.*REVIEW_SET/s);
  });
});
