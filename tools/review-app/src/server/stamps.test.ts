import { describe, expect, it } from "vite-plus/test";
import {
  diffStamps,
  hasChange,
  loadedStamps,
  stampsOf,
  watchTargets,
  watchTargetsKey,
  type Stamps,
} from "./stamps";
import type { ReviewSet } from "./reviewSet";

function stamps(set: number, assets: Record<string, number>): Stamps {
  // Tests speak in mtimes; the stamp is `mtime:size` and size is constant here.
  const composed: Record<string, string> = {};
  for (const [path, mtime] of Object.entries(assets)) composed[path] = `${String(mtime)}:10`;
  return { set: `${String(set)}:10`, assets: composed };
}

describe("diffStamps", () => {
  it("sees nothing when nothing moved", () => {
    const before = stamps(10, { "/s/a.jpg": 1, "/s/b.jpg": 2 });
    expect(hasChange(diffStamps(before, stamps(10, { "/s/a.jpg": 1, "/s/b.jpg": 2 })))).toBe(false);
  });

  it("names only the rendition that was re-rendered", () => {
    const before = stamps(10, { "/s/a.jpg": 1, "/s/b.jpg": 2 });
    const after = stamps(10, { "/s/a.jpg": 1, "/s/b.jpg": 99 });
    expect(diffStamps(before, after)).toEqual({
      setChanged: false,
      changedAssets: ["/s/b.jpg"],
    });
  });

  it("reports the review document separately from its renditions", () => {
    const diff = diffStamps(stamps(10, { "/s/a.jpg": 1 }), stamps(11, { "/s/a.jpg": 1 }));
    expect(diff).toEqual({ setChanged: true, changedAssets: [] });
  });

  it("sees a rewrite that kept the same mtime", () => {
    // `cp -p`, `rsync -t` and a coarse filesystem clock all leave the mtime
    // alone, and an unnoticed rewrite is the page serving the previous render
    // from an `immutable` cache entry.
    const before: Stamps = { set: "10:1", assets: { "/s/a.jpg": "500:1200" } };
    const after: Stamps = { set: "10:1", assets: { "/s/a.jpg": "500:1600" } };
    expect(diffStamps(before, after).changedAssets).toEqual(["/s/a.jpg"]);
  });

  it("counts a rendition appearing or vanishing as a change", () => {
    // 0 is how a file that is not there stamps, so a render finally landing —
    // the case the watcher exists for — has to register.
    const appeared = diffStamps(stamps(10, { "/s/a.jpg": 0 }), stamps(10, { "/s/a.jpg": 7 }));
    expect(appeared.changedAssets).toEqual(["/s/a.jpg"]);

    // A `review.json` edit can add or drop renditions outright.
    const added = diffStamps(stamps(10, {}), stamps(10, { "/s/new.jpg": 7 }));
    expect(added.changedAssets).toEqual(["/s/new.jpg"]);
    const dropped = diffStamps(stamps(10, { "/s/old.jpg": 7 }), stamps(10, {}));
    expect(dropped.changedAssets).toEqual(["/s/old.jpg"]);
  });
});

describe("watchTargets", () => {
  it("covers renditions in subdirectories with the recursive watch", () => {
    const targets = watchTargets("/s", ["/s/a.jpg", "/s/renders/b.jpg", "/s/deep/er/c.jpg"]);
    expect(targets).toEqual({ recursive: "/s", others: [] });
  });

  it("also watches a directory the set points outside itself", () => {
    // The asset map allows this deliberately, so the watcher has to follow.
    const targets = watchTargets("/s", ["/s/a.jpg", "/other/renders/b.jpg"]);
    expect(targets).toEqual({ recursive: "/s", others: ["/other/renders"] });
  });

  it("does not watch one outside directory twice", () => {
    const targets = watchTargets("/s", ["/other/b.jpg", "/other/c.jpg", "/other/d.jpg"]);
    expect(targets.others).toEqual(["/other"]);
  });

  it("is not fooled by a sibling directory sharing a name prefix", () => {
    // `/sets-old` starts with `/set` but is not inside `/set`.
    const targets = watchTargets("/set", ["/set/a.jpg", "/sets-old/b.jpg"]);
    expect(targets.others).toEqual(["/sets-old"]);
  });
});

describe("loadedStamps", () => {
  it("reports the mtimes the loaded set carries, not what is on disk now", () => {
    // The baseline for a starting watch. Taking it from disk would record a
    // rendition rewritten just before the watcher started as already-seen: the
    // model would keep serving its old URL and no later diff would mention it.
    const set = {
      path: "/s/review.json",
      assets: { entries: () => [{ path: "/s/a.jpg", mtimeMs: 100, size: 10 }] },
    } as unknown as ReviewSet;

    const baseline = loadedStamps(set, () => ({ mtimeMs: 999, size: 99 }));
    expect(baseline.assets["/s/a.jpg"]).toBe("100:10");

    // So the drift is visible rather than swallowed.
    const now = stampsOf(set, () => ({ mtimeMs: 999, size: 99 }));
    expect(hasChange(diffStamps(baseline, now))).toBe(true);
  });
});

describe("watchTargetsKey", () => {
  it("changes when a set starts naming a directory nothing watched", () => {
    // A `review.json` edit can move a rendition outside the set directory; the
    // watchers must follow it, or renders there are never reported.
    const before = watchTargetsKey(watchTargets("/s", ["/s/a.jpg"]));
    const after = watchTargetsKey(watchTargets("/s", ["/s/a.jpg", "/elsewhere/b.jpg"]));
    expect(after).not.toBe(before);
  });

  it("is stable when the same directories are named in a different order", () => {
    const one = watchTargetsKey(watchTargets("/s", ["/x/a.jpg", "/y/b.jpg"]));
    const two = watchTargetsKey(watchTargets("/s", ["/y/b.jpg", "/x/a.jpg"]));
    expect(two).toBe(one);
  });
});
