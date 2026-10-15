import { readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";
import {
  chooseByAnchor,
  failingIds,
  insideQuad,
  labelProblems,
  mergeChecked,
  seedLabels,
  type CheckResult,
  fitLongSide,
  homography,
  orderClockwise,
  project,
  projectPatches,
  rotate,
  targetToFrame,
  type Quad,
  type Target,
} from "./chart";

const classic = JSON.parse(
  readFileSync(
    new URL("../../../scripts/analysis/nctool/targets/ccpp2-classic.json", import.meta.url),
    "utf8",
  ),
) as Target;

const CORNERS: Quad = [
  [0.2, 0.15],
  [0.8, 0.2],
  [0.78, 0.85],
  [0.18, 0.8],
];

describe("projection", () => {
  it("matches nctool.charts on the shared case", () => {
    // test_charts.py projects the same point through the same corners.
    const [x, y] = project(targetToFrame(classic, CORNERS), [20, 10]);
    expect(x).toBeCloseTo(0.3344222625529341, 12);
    expect(y).toBeCloseTo(0.2755293405928615, 12);
  });

  it("maps the target's corners onto the label's", () => {
    const h = targetToFrame(classic, CORNERS);
    const [w, hh] = classic.size;
    const got = [project(h, [0, 0]), project(h, [w, 0]), project(h, [w, hh]), project(h, [0, hh])];
    got.forEach((p, i) => {
      expect(p[0]).toBeCloseTo(CORNERS[i]![0], 12);
      expect(p[1]).toBeCloseTo(CORNERS[i]![1], 12);
    });
  });

  it("refuses degenerate corners", () => {
    expect(() =>
      homography(
        [
          [0, 0],
          [1, 0],
          [2, 0],
          [0, 1],
        ],
        CORNERS,
      ),
    ).toThrow(/degenerate/);
    expect(
      projectPatches(classic, [CORNERS[0], CORNERS[0], CORNERS[0], CORNERS[0]]),
    ).toBeUndefined();
  });

  it("projects every patch and its sampled middle", () => {
    const patches = projectPatches(classic, CORNERS)!;
    expect(patches).toHaveLength(24);
    const a1 = patches.find((p) => p.id === "A1")!;
    expect(a1.quad[0][0]).toBeCloseTo(0.2, 12);
    expect(a1.sample[0][0]).toBeGreaterThan(a1.quad[0][0]);
    expect(patches.filter((p) => p.neutral).map((p) => p.id)).toEqual(classic.neutrals);
  });
});

describe("labels", () => {
  it("rotates a quarter turn and back in four", () => {
    expect(rotate(CORNERS)[0]).toEqual(CORNERS[3]);
    expect(rotate(rotate(rotate(rotate(CORNERS))))).toEqual(CORNERS);
  });

  it("reads the patch ids a failure names", () => {
    const ids = failingIds([
      "A1 not uniform (spread 0.80 stops)",
      "gap M1a|right reads 1.20 stops brighter than black",
      "neutrals out of order at N3->N4 (-0.30 stops)",
      "F1 shows no colour (chroma 0.10 stops)",
    ]);
    expect([...ids].sort()).toEqual(["A1", "F1", "M1a", "N3", "N4"]);
  });

  it("orders four clicks clockwise from the one nearest the top left, whatever their order", () => {
    const shuffled = [CORNERS[2], CORNERS[0], CORNERS[3], CORNERS[1]];
    expect(orderClockwise(shuffled, 1000, 800)).toEqual(CORNERS);
    expect(orderClockwise([...CORNERS].reverse(), 1000, 800)).toEqual(CORNERS);
  });

  it("picks, of several passing orientations, the one with the anchor under the click", () => {
    const upsideDown = rotate(rotate(CORNERS));
    const white = projectPatches(classic, CORNERS)!.find((p) => p.id === classic.anchor.patch)!;
    const click = [
      white.quad.reduce((s, p) => s + p[0], 0) / 4,
      white.quad.reduce((s, p) => s + p[1], 0) / 4,
    ] as const;
    expect(chooseByAnchor(classic, [upsideDown, CORNERS], click)).toEqual(CORNERS);
    expect(chooseByAnchor(classic, [upsideDown], click)).toBeUndefined();
    expect(insideQuad(CORNERS, [0.5, 0.5])).toBe(true);
    expect(insideQuad(CORNERS, [0.9, 0.5])).toBe(false);
  });

  it("refuses a label the labels file could not hold", () => {
    const label = { target: "ccpp2-classic", card: "ccpp2", corners: CORNERS };
    expect(labelProblems(label)).toEqual([]);
    const crossed: Quad = [CORNERS[0], CORNERS[2], CORNERS[1], CORNERS[3]];
    expect(labelProblems({ ...label, corners: crossed })[0]).toMatch(/convex/);
    const outside: Quad = [[1.2, 0], CORNERS[1], CORNERS[2], CORNERS[3]];
    expect(labelProblems({ ...label, corners: outside })[0]).toMatch(/inside the frame/);
    expect(labelProblems({ ...label, card: " " })).toEqual(["card must not be empty"]);
  });

  it("merges a check's corners only into the labels that were sent", () => {
    const sent = [{ target: "ccpp2-classic", card: "ccpp2", corners: CORNERS }];
    const turned = rotate(rotate(CORNERS));
    const result = { corners: turned } as unknown as CheckResult;
    expect(mergeChecked(sent, sent, [result])).toEqual({
      labels: [{ ...sent[0], corners: turned }],
      clean: true,
    });
    // A corner moved while the check ran: keep it, and stay unsaved.
    const moved = [{ ...sent[0]!, corners: rotate(CORNERS) }];
    expect(mergeChecked(sent, moved, [result])).toEqual({ labels: moved, clean: false });
  });

  it("seeds a frame from the previous one on its first visit only", () => {
    const prev = [{ target: "ccpp2-classic", card: "ccpp2", corners: CORNERS }];
    expect(seedLabels(true, [], prev)).toEqual(prev);
    expect(seedLabels(false, [], prev)).toBeUndefined(); // cleared on purpose
    expect(seedLabels(true, prev, prev)).toBeUndefined(); // already labelled
    expect(seedLabels(true, [], undefined)).toBeUndefined();
  });

  it("lays the target's long side along the outline's, in frame pixels", () => {
    // The Classic is wide (87 x 57). Upright on a portrait frame, the outline is tall.
    const tall: Quad = [
      [0.3, 0.1],
      [0.7, 0.1],
      [0.7, 0.9],
      [0.3, 0.9],
    ];
    expect(fitLongSide(classic, tall, 1000, 1000)).toEqual(rotate(tall));
    expect(fitLongSide(classic, CORNERS, 1000, 800)).toEqual(CORNERS);
    // The same fractions on a frame four times as wide are a wide outline.
    expect(fitLongSide(classic, tall, 4000, 1000)).toEqual(tall);
  });
});
