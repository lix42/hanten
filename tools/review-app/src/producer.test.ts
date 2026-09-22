import { describe, expect, it } from "vite-plus/test";
import { producerSummary, shortCommit, type Producer } from "./producer";

const BUILD: Producer = {
  kind: "hanten",
  label: "candidate",
  ncVersion: "0.1.0",
  gitCommit: "2664a0ddbdd5",
  gitDirty: false,
  pipelineVersion: 5,
  target: "aarch64-apple-darwin",
};

describe("shortCommit", () => {
  // nc stamps twelve; seven is what pastes back into `git show`.
  it("takes the conventional seven characters", () => {
    expect(shortCommit("2664a0ddbdd5")).toBe("2664a0d");
  });

  it("passes a shorter value through rather than padding it", () => {
    expect(shortCommit("abc")).toBe("abc");
  });
});

describe("producerSummary", () => {
  it("leads with the commit, which is what two builds are told apart by", () => {
    expect(producerSummary(BUILD)).toBe("2664a0d · pipeline 5 · 0.1.0 · aarch64-apple-darwin");
  });

  // The generator accepts a pre-rename `nc` banner on purpose — that binary is the
  // usual reference arm — so the line must not claim every build is `hanten`.
  it("names no product beside the version", () => {
    expect(producerSummary(BUILD)).not.toContain("hanten");
  });

  // The one field that means the binary is not reproducible from what the commit
  // names — which for a patched-versus-shipped comparison is the point of the cell.
  it("says a dirty tree out loud", () => {
    expect(producerSummary({ ...BUILD, gitDirty: true })).toContain("dirty tree");
  });

  // A binary built from a tarball stamps no commit. An incomplete label still
  // beats no label.
  it("says so when there is no commit", () => {
    const { gitCommit: _omitted, ...rest } = BUILD;
    expect(producerSummary(rest)).toBe(
      "commit unknown · pipeline 5 · 0.1.0 · aarch64-apple-darwin",
    );
  });

  it("carries the matrix's own note last", () => {
    expect(producerSummary({ ...BUILD, note: "built from the v0 tag" })).toMatch(
      /built from the v0 tag$/,
    );
  });

  it("has something to say about a build that identified itself nowhere", () => {
    expect(producerSummary({ kind: "hanten", label: "only" })).toBe("commit unknown");
  });

  // `analysis/review-reference-cells`: an outside producer's cell is a variant of
  // this block, not a second one.
  it("describes an external producer by its note", () => {
    expect(producerSummary({ kind: "external", label: "NLP", note: "nlp/2026-09-09" })).toBe(
      "nlp/2026-09-09",
    );
    expect(producerSummary({ kind: "external", label: "NLP" })).toBe("not rendered by hanten");
  });
});
