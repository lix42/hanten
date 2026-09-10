/**
 * The one review set this server is serving.
 *
 * Held rather than re-read per request because the image route has to resolve
 * `/img/<id>` against the same asset map the page was rendered from. It is
 * re-read whenever `review.json`'s own mtime changes.
 *
 * A *rendition* changing does not move that mtime, and its mtime is frozen into
 * the asset map at parse time — so the watcher must call `invalidateReviewSet`
 * for those too, or every `/img/` URL keeps its old `?v=` and the page goes on
 * showing the previous render behind an `immutable` cache entry.
 *
 * A failed load is never cached: the next request retries, so fixing a typo in
 * the path or the JSON needs a reload rather than a restart.
 */

import { statSync } from "node:fs";
import { loadReviewSet, resolveSetFile, resolveSetPath, type ReviewSet } from "./reviewSet";

let cached: { set: ReviewSet; stamp: number } | undefined;

function stampOf(path: string): number {
  try {
    return statSync(path).mtimeMs;
  } catch {
    return 0;
  }
}

export async function currentReviewSet(
  env: Record<string, string | undefined> = process.env,
  cwd: string = process.cwd(),
): Promise<ReviewSet> {
  const setPath = resolveSetPath(env, cwd);
  // Compare the *resolved* file, which is what the loaded set records. Keying on
  // the stated path missed the cache forever whenever a set was named by its
  // directory.
  const file = resolveSetFile(setPath);
  const stamp = stampOf(file);
  if (cached && cached.set.path === file && cached.stamp === stamp) return cached.set;
  const set = await loadReviewSet(setPath);
  cached = { set, stamp };
  return set;
}

/** Drop the held set so the next request re-reads it. */
export function invalidateReviewSet(): void {
  cached = undefined;
}
