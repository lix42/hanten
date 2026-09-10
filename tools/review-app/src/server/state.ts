/**
 * The one review set this server is serving.
 *
 * Held rather than re-read per request because the image route has to resolve
 * `/img/<id>` against the same asset map the page was rendered from. It is
 * re-read whenever `review.json`'s own mtime changes, so editing the file and
 * reloading works without a restart. A *rendition* changing does not move that
 * mtime — keeping those URLs fresh is the watcher's job.
 *
 * A failed load is never cached: the next request retries, so fixing a typo in
 * the path or the JSON needs a reload rather than a restart.
 */

import { statSync } from "node:fs";
import { loadReviewSet, resolveSetPath, type ReviewSet } from "./reviewSet";

let cached: { set: ReviewSet; stamp: number } | undefined;

function stampOf(path: string): number {
  try {
    return statSync(path).mtimeMs;
  } catch {
    return 0;
  }
}

export async function currentReviewSet(): Promise<ReviewSet> {
  const setPath = resolveSetPath(process.env, process.cwd());
  const stamp = stampOf(setPath.path);
  if (cached && cached.set.path === setPath.path && cached.stamp === stamp) return cached.set;
  const set = await loadReviewSet(setPath);
  cached = { set, stamp };
  return set;
}

/** Drop the held set so the next request re-reads it. */
export function invalidateReviewSet(): void {
  cached = undefined;
}
