import { mkdtempSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vite-plus/test";
import { SYNTHETIC_METRICS } from "../charts/fixture";
import { SET_ENV_VAR } from "./reviewSet";
import { currentReviewSet, invalidateReviewSet } from "./state";

/**
 * Covers the two ways the held set went wrong. Both were invisible to every
 * other test: the modules that carried them do filesystem and process work, and
 * the pure stamp tests prove a change is *detected* without proving the
 * detection ever reaches a URL.
 */

function makeSet(measured = false): { dir: string; file: string; image: string; record: string } {
  const dir = mkdtempSync(join(tmpdir(), "nc-review-set-"));
  const image = join(dir, "E1-a.svg");
  writeFileSync(image, "<svg xmlns='http://www.w3.org/2000/svg'/>");
  const record = join(dir, "E1-a.metrics.json");
  if (measured) writeFileSync(record, JSON.stringify(SYNTHETIC_METRICS));
  const file = join(dir, "review.json");
  writeFileSync(
    file,
    JSON.stringify({
      schema_version: 1,
      configs: [{ id: "a", label: "a" }],
      images: [
        {
          id: "E1",
          renditions: {
            a: measured ? { src: "E1-a.svg", metrics: "E1-a.metrics.json" } : "E1-a.svg",
          },
        },
      ],
    }),
  );
  return { dir, file, image, record };
}

const srcOf = (set: Awaited<ReturnType<typeof currentReviewSet>>) =>
  set.review.images[0]!.renditions["a"]!.src;

afterEach(() => {
  invalidateReviewSet();
});

describe("currentReviewSet", () => {
  it("caches a set named by its directory, like one named by its file", async () => {
    // The cache compared the *stated* path against the loaded set's *resolved*
    // one, so the directory form never matched and every request — including
    // every `/img/` request — re-read and re-parsed the whole set.
    const { dir, file } = makeSet();
    for (const stated of [dir, file]) {
      invalidateReviewSet();
      const first = await currentReviewSet({ [SET_ENV_VAR]: stated }, "/");
      const second = await currentReviewSet({ [SET_ENV_VAR]: stated }, "/");
      expect(second).toBe(first);
      expect(first.path).toBe(file);
    }
  });

  it("gives a re-rendered file a new URL once the held set is dropped", async () => {
    // A rendition's mtime is frozen into the asset map at parse time and is what
    // the `/img/` URL carries, so a held set keeps addressing the old bytes —
    // which the browser then serves from an `immutable` cache entry.
    const { file, image } = makeSet();
    const env = { [SET_ENV_VAR]: file };

    const before = srcOf(await currentReviewSet(env, "/"));
    const later = new Date(Date.now() + 5000);
    utimesSync(image, later, later);

    // Without the drop the stale URL survives, which is the bug.
    expect(srcOf(await currentReviewSet(env, "/"))).toBe(before);

    invalidateReviewSet();
    expect(srcOf(await currentReviewSet(env, "/"))).not.toBe(before);
  });
});

describe("a set that names measurements", () => {
  it("reads each record into the model and lists it for the watcher", async () => {
    const { file, record } = makeSet(true);
    const set = await currentReviewSet({ [SET_ENV_VAR]: file }, "/");

    expect(set.review.images[0]!.renditions["a"]!.metrics?.cast.length).toBeGreaterThan(0);
    // Listed, or a re-measurement never reaches the page: the model holds the
    // parsed record, so nothing re-reads it until the held set is dropped.
    expect(set.data.map((file) => file.path)).toEqual([record]);
    expect(set.data[0]!.mtimeMs).toBeGreaterThan(0);
  });

  it("leaves an unmeasured set alone", async () => {
    const { file } = makeSet();
    const set = await currentReviewSet({ [SET_ENV_VAR]: file }, "/");
    expect(set.review.images[0]!.renditions["a"]!.metrics).toBeUndefined();
    expect(set.data).toEqual([]);
  });
});
