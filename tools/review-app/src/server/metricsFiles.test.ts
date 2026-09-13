import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vite-plus/test";
import { SYNTHETIC_METRICS } from "../charts/fixture";
import { createMetricsFiles } from "./metricsFiles";
import type { StatFile } from "./assets";

/**
 * Reads real files, like the set loader does. The parsing itself is covered in
 * `charts/metrics.test.ts`; what is covered here is the part that only exists
 * server-side — resolving against the set's directory, surviving a record that
 * cannot be read, and recording what the watcher has to stamp.
 */

const stat: StatFile = () => ({ mtimeMs: 500, size: 42 });

function setDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "nc-review-metrics-"));
  writeFileSync(join(dir, "good.metrics.json"), JSON.stringify(SYNTHETIC_METRICS));
  writeFileSync(join(dir, "truncated.metrics.json"), "{ not json");
  writeFileSync(join(dir, "wrong.metrics.json"), JSON.stringify({ schema_version: 1 }));
  return dir;
}

describe("createMetricsFiles", () => {
  it("resolves against the review file's directory and parses the record", () => {
    const dir = setDir();
    const files = createMetricsFiles(dir, stat);
    const result = files.load("good.metrics.json", "images[0].renditions.a.metrics");
    expect("metrics" in result && result.metrics.cast.length).toBeGreaterThan(0);
    expect(files.files()).toEqual([
      { path: join(dir, "good.metrics.json"), mtimeMs: 500, size: 42 },
    ]);
  });

  it("names the document path and the fault when a record will not parse", () => {
    const files = createMetricsFiles(setDir(), stat);
    for (const name of ["truncated.metrics.json", "wrong.metrics.json", "absent.json"]) {
      const result = files.load(name, `images[0].renditions.${name}.metrics`);
      expect("metricsError" in result).toBe(true);
      if ("metricsError" in result) expect(result.metricsError).toContain("renditions");
    }
  });

  // A measurement still running is the ordinary case: the generator writes the
  // image first. The record has to be watched anyway, or the page never notices
  // it arriving — the same rule renditions follow.
  it("records a file that is not there yet, so the watcher sees it appear", () => {
    const dir = setDir();
    const files = createMetricsFiles(dir, () => undefined);
    files.load("not-yet.metrics.json", "at");
    expect(files.files()).toEqual([
      { path: join(dir, "not-yet.metrics.json"), mtimeMs: 0, size: 0 },
    ]);
  });

  it("records one file once however many renditions name it", () => {
    const files = createMetricsFiles(setDir(), stat);
    files.load("good.metrics.json", "a");
    files.load("good.metrics.json", "b");
    expect(files.files()).toHaveLength(1);
  });
});
