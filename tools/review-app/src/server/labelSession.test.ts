import { describe, expect, it } from "vite-plus/test";
import { findRender, labelConfig, selectFrames, withFrame } from "./labelSession";

const manifest = {
  rolls: {
    R: {
      frames: [
        { file: "rolls/R/2126.tif", role: "real" },
        { file: "rolls/R/2127.tif", role: "chart" },
        { file: "rolls/R/2128.tif", role: "chart" },
      ],
    },
  },
};

describe("selectFrames", () => {
  it("takes the roll's chart frames by default", () => {
    const got = selectFrames(manifest, "R", []);
    expect(Array.isArray(got) && got.map((f) => f.file)).toEqual([
      "rolls/R/2127.tif",
      "rolls/R/2128.tif",
    ]);
  });

  it("takes named frames, in the order named, whatever their role", () => {
    const got = selectFrames(manifest, "R", ["2128", "2126"]);
    expect(Array.isArray(got) && got.map((f) => f.file)).toEqual([
      "rolls/R/2128.tif",
      "rolls/R/2126.tif",
    ]);
  });

  it("names what is missing", () => {
    expect(selectFrames(manifest, "Q", [])).toMatch(/no roll "Q"/);
    expect(selectFrames(manifest, "R", ["9999"])).toMatch(/no frame "9999"/);
  });
});

describe("findRender", () => {
  it("prefers a browser format and matches the stem exactly", () => {
    expect(findRender("/r", "2127", ["21270.jpg", "2127.tif", "2127.JPG"])).toBe("/r/2127.JPG");
    expect(findRender("/r", "2127", ["2127.tif"])).toBe("/r/2127.tif");
    expect(findRender("/r", "2127", ["2128.jpg"])).toBeUndefined();
  });
});

describe("withFrame", () => {
  const label = {
    target: "ccpp2-classic",
    card: "ccpp2",
    corners: [
      [0, 0],
      [1, 0],
      [1, 1],
      [0, 1],
    ],
  } as const;

  it("replaces one frame, keeps the others, and sorts", () => {
    const file = { roll: "R", frames: { "2128": [label] } };
    const next = withFrame(file, "2127", [label]);
    expect(Object.keys(next.frames)).toEqual(["2127", "2128"]);
    expect(withFrame(next, "2127", []).frames).toEqual({ "2128": [label] });
  });
});

describe("labelConfig", () => {
  it("names every missing variable", () => {
    expect(labelConfig({ LABEL_ROLL: "R" })).toMatch(
      /LABEL_RENDERS, LABEL_FILE, LABEL_ASSET_ROOT, LABEL_REPO/,
    );
  });

  it("defaults the card and splits the frames", () => {
    const got = labelConfig({
      LABEL_ROLL: "R",
      LABEL_RENDERS: "/r",
      LABEL_FILE: "/l.json",
      LABEL_ASSET_ROOT: "/a",
      LABEL_REPO: "/nonexistent",
      LABEL_FRAMES: "2127,2128",
    });
    expect(typeof got !== "string" && [got.card, got.frames, got.python]).toEqual([
      "ccpp2",
      ["2127", "2128"],
      "python3",
    ]);
  });
});
