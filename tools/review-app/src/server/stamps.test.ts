import { describe, expect, it } from "vite-plus/test";
import { diffStamps, hasChange, watchTargets, type Stamps } from "./stamps";

function stamps(set: number, assets: Record<string, number>): Stamps {
  return { set, assets };
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
