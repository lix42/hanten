import { describe, expect, it } from "vite-plus/test";
import { BUNDLED_EXAMPLE, resolveSetPath, setFilePath, SET_ENV_VAR } from "./reviewSet";

const CWD = "/work/review-app";

describe("resolveSetPath", () => {
  it("falls back to the bundled example when nothing is stated", () => {
    for (const env of [{}, { [SET_ENV_VAR]: "" }, { [SET_ENV_VAR]: "   " }]) {
      const resolved = resolveSetPath(env, CWD);
      expect(resolved.source).toBe("bundled-example");
      expect(resolved.path).toBe(`${CWD}/${BUNDLED_EXAMPLE}`);
    }
  });

  it("takes an absolute REVIEW_SET as given", () => {
    const resolved = resolveSetPath({ [SET_ENV_VAR]: "/sets/tone/review.json" }, CWD);
    expect(resolved).toEqual({ path: "/sets/tone/review.json", source: "env" });
  });

  it("resolves a relative REVIEW_SET against the directory the server started in", () => {
    expect(resolveSetPath({ [SET_ENV_VAR]: "../sets/tone/review.json" }, CWD).path).toBe(
      "/work/sets/tone/review.json",
    );
  });
});

describe("setFilePath", () => {
  it("reads the review.json inside a directory", () => {
    expect(setFilePath("/sets/tone", true)).toBe("/sets/tone/review.json");
  });

  it("leaves a file path alone", () => {
    expect(setFilePath("/sets/tone/review.json", false)).toBe("/sets/tone/review.json");
  });
});
