/**
 * Reading the metric records a review set names.
 *
 * A record is a sibling file rather than a block inside `review.json`: it is
 * ~20 kB of counts per rendition, it is written by a different tool at a
 * different time, and keeping it separate is what lets a re-measurement update
 * the page without rewriting the review document.
 *
 * The files are **read here and parsed into the model**, so what crosses the
 * wire is the charted subset rather than the whole record — and the client never
 * learns a filesystem path. They are deliberately *not* registered in the asset
 * map: that map is the set of files this server may serve, and a metrics record
 * has no business behind an `/img/` URL.
 */

import { readFileSync } from "node:fs";
import { resolve as resolvePath } from "node:path";
import { parseMetrics } from "../charts/metrics";
import type { Asset, StatFile } from "./assets";
import type { LoadMetrics } from "../review";

export interface MetricsFiles {
  /** The `LoadMetrics` to hand `parseReview`. */
  readonly load: LoadMetrics;
  /**
   * Every record read, with the mtime and size it was read at.
   *
   * Shaped as an `Asset` so the watcher stamps it exactly like a rendition: a
   * re-measurement has to reach the page, and the stamp taken at load time is
   * what makes a rewrite between load and first watch visible rather than
   * silently already-seen.
   */
  files(): readonly Asset[];
}

export function createMetricsFiles(dir: string, stat: StatFile): MetricsFiles {
  const seen = new Map<string, Asset>();
  return {
    load(path, at) {
      const absolute = resolvePath(dir, path);
      const stats = stat(absolute);
      // Recorded even when the read fails, so a record that appears later — a
      // measurement still running — is noticed by the watcher and picked up.
      seen.set(absolute, {
        path: absolute,
        mtimeMs: stats?.mtimeMs ?? 0,
        size: stats?.size ?? 0,
      });
      try {
        return { metrics: parseMetrics(JSON.parse(readFileSync(absolute, "utf8"))) };
      } catch (cause) {
        return { metricsError: `${at}: ${describe(cause, path)}` };
      }
    },
    files: () => [...seen.values()],
  };
}

/**
 * A read or parse failure, as the page should show it.
 *
 * Node embeds the **absolute** path in an I/O error, so passing `message`
 * through would put the server's filesystem layout in the page — which the
 * header above promises it does not. The path the set itself wrote is the
 * useful half anyway, and a parse failure already names the offending field.
 */
function describe(cause: unknown, path: string): string {
  if (cause instanceof Error && "code" in cause && typeof cause.code === "string") {
    return `${cause.code} reading ${path}`;
  }
  return cause instanceof Error ? cause.message : String(cause);
}
