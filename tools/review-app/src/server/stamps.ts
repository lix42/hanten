/**
 * Deciding whether anything the page is showing has actually changed.
 *
 * A filesystem event says only that *something* happened nearby, and one save
 * can produce several. Rather than interpret event paths — which would have to
 * cope with renames, atomic-replace writes and editors' temp files — the
 * watcher re-stats what the set is made of and compares. That is a handful of
 * `stat` calls per burst against tens of files, and it cannot be fooled by an
 * event arriving for a path that is not the one that matters.
 */

import { dirname, sep } from "node:path";
import type { StatMtime } from "./assets";
import type { ReviewSet } from "./reviewSet";

export interface Stamps {
  /** mtime of the `review.json` itself; 0 when it has gone missing. */
  readonly set: number;
  /** mtime per absolute rendition path. 0 for one whose file is not there. */
  readonly assets: Readonly<Record<string, number>>;
}

export interface StampDiff {
  /** The review document changed, so the whole model must be re-read. */
  readonly setChanged: boolean;
  /** Paths that appeared, vanished or were rewritten. */
  readonly changedAssets: readonly string[];
}

export function diffStamps(before: Stamps, after: Stamps): StampDiff {
  const paths = new Set([...Object.keys(before.assets), ...Object.keys(after.assets)]);
  const changedAssets = [...paths].filter((p) => before.assets[p] !== after.assets[p]).sort();
  return { setChanged: before.set !== after.set, changedAssets };
}

export function hasChange(diff: StampDiff): boolean {
  return diff.setChanged || diff.changedAssets.length > 0;
}

/**
 * Directories to watch for one set.
 *
 * The set's own directory is watched recursively, which covers renditions in
 * subdirectories. A set may also name files outside it (`../renders/E1.jpg`) —
 * the asset map allows that deliberately — so each such directory is watched
 * too, and any that a recursive ancestor already covers is dropped rather than
 * watched twice.
 */
export interface WatchTargets {
  readonly recursive: string;
  readonly others: readonly string[];
}

export function watchTargets(setDir: string, assetPaths: readonly string[]): WatchTargets {
  const others = new Set<string>();
  for (const path of assetPaths) {
    const dir = dirname(path);
    if (!isUnder(dir, setDir)) others.add(dir);
  }
  return { recursive: setDir, others: [...others].sort() };
}

function isUnder(path: string, ancestor: string): boolean {
  return path === ancestor || path.startsWith(ancestor.endsWith(sep) ? ancestor : ancestor + sep);
}

export function stampsOf(set: ReviewSet, stat: StatMtime): Stamps {
  const assets: Record<string, number> = {};
  for (const asset of set.assets.entries()) assets[asset.path] = stat(asset.path) ?? 0;
  return { set: stat(set.path) ?? 0, assets };
}
