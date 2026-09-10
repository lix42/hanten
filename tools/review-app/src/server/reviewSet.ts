/**
 * Loading the review set the server was pointed at.
 *
 * The set is named once, when the server starts, by the `REVIEW_SET`
 * environment variable — `pnpm dev <path>` sets it for you. That is what
 * replaced the old `?data=` URL: the path is read from disk, so it can live
 * anywhere, and nothing has to be copied next to the app or reachable from a
 * served root.
 */

import { readFile } from "node:fs/promises";
import { statSync } from "node:fs";
import { dirname, isAbsolute, resolve as resolvePath } from "node:path";
import { parseReview, type Review } from "../review";
import { createAssetMap, type AssetMap, type StatMtime } from "./assets";

/** Env var naming the review set. */
export const SET_ENV_VAR = "REVIEW_SET";

/**
 * Path of the review set to render, and whether it was chosen or fallen back to.
 *
 * With nothing stated the committed synthetic example is used, so a bare
 * `pnpm dev` still shows a working page — the same reason that example is in
 * the repo at all.
 */
export const BUNDLED_EXAMPLE = "public/examples/synthetic/review.json";

export interface SetPath {
  readonly path: string;
  readonly source: "env" | "bundled-example";
}

export function resolveSetPath(env: Record<string, string | undefined>, cwd: string): SetPath {
  const stated = env[SET_ENV_VAR]?.trim();
  if (!stated) return { path: resolvePath(cwd, BUNDLED_EXAMPLE), source: "bundled-example" };
  // A relative `REVIEW_SET` is resolved against the directory the server was
  // started in, which is what a shell-completed path means.
  return { path: isAbsolute(stated) ? stated : resolvePath(cwd, stated), source: "env" };
}

/**
 * A path naming a directory means the `review.json` inside it.
 *
 * Pointing at the set's folder is the natural thing to type — and to tab-complete
 * — so it is accepted rather than answered with "no such file".
 */
export function setFilePath(path: string, isDirectory: boolean): string {
  return isDirectory ? resolvePath(path, "review.json") : path;
}

/**
 * The `review.json` a stated path names, following the directory form.
 *
 * Exported because the cache key must be computed the same way the loader
 * computes it: keying on the *stated* path while storing the *resolved* one
 * meant a directory-form set never matched its own cache entry, so it was
 * re-read and re-parsed on every request — and, worse, made the two documented
 * ways of naming a set behave differently.
 */
export function resolveSetFile(setPath: SetPath): string {
  return setFilePath(setPath.path, isDirectory(setPath.path));
}

export interface ReviewSet {
  readonly review: Review;
  readonly assets: AssetMap;
  /** Absolute path of the `review.json` itself. */
  readonly path: string;
  /** Directory rendition paths resolve against, and the watcher watches. */
  readonly dir: string;
  readonly source: SetPath["source"];
}

const mtime: StatMtime = (path) => {
  try {
    return statSync(path).mtimeMs;
  } catch {
    return undefined;
  }
};

/**
 * Read, parse and resolve a review set.
 *
 * A rendition naming a file that does not exist is *not* fatal: a config whose
 * render failed is ordinary, and refusing the whole set would hide the
 * comparisons that did work. It registers with an mtime of 0 and its request
 * 404s, which the page already renders as a visible gap.
 */
export async function loadReviewSet(setPath: SetPath, stat: StatMtime = mtime): Promise<ReviewSet> {
  const path = resolveSetFile(setPath);
  const dir = dirname(path);
  const assets = createAssetMap(stat);

  let text: string;
  try {
    text = await readFile(path, "utf8");
  } catch (cause) {
    throw new Error(
      `could not read the review set at ${path}: ${describeCause(cause)}. ` +
        `Point ${SET_ENV_VAR} at a review.json, or run \`pnpm dev <path to review.json>\`.`,
    );
  }

  let json: unknown;
  try {
    json = JSON.parse(text);
  } catch (cause) {
    throw new Error(`${path} is not valid JSON: ${describeCause(cause)}`);
  }

  const review = parseReview(json, (path) => assets.register(resolvePath(dir, path)));
  return { review, assets, path, dir, source: setPath.source };
}

function isDirectory(path: string): boolean {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
}

function describeCause(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
