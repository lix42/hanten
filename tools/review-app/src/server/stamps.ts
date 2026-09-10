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
import type { StatFile } from "./assets";
import type { ReviewSet } from "./reviewSet";

/**
 * `mtime:size`, not an mtime alone.
 *
 * A rewrite does not always move the mtime — a timestamp-preserving copy
 * (`cp -p`, `rsync -t`) and a coarse filesystem clock both leave it be — and an
 * unnoticed rewrite is the page serving the previous render from an `immutable`
 * cache entry. Size is not a checksum, but it is free here and catches the
 * cases mtime misses.
 */
function stamp(mtimeMs: number, size: number): string {
  return `${String(mtimeMs)}:${String(size)}`;
}

export interface Stamps {
  /** The `review.json` itself; `0:0` when it has gone missing. */
  readonly set: string;
  /** Per absolute rendition path. `0:0` for one whose file is not there. */
  readonly assets: Readonly<Record<string, string>>;
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

/** Stable identity of a target set, for noticing when the watch must move. */
export function watchTargetsKey(targets: WatchTargets): string {
  return [targets.recursive, ...targets.others].join("\u0000");
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

/**
 * Stamps as the **loaded set** sees them — the mtimes its `/img/` URLs actually
 * carry, not what is on disk now.
 *
 * This is the right baseline when the watch starts, because a rendition
 * rewritten between the page's first read and the watcher's first breath would
 * otherwise be recorded as already-seen: the model would keep serving the old
 * URL, no later diff would ever mention it, and the page would sit silently
 * stale. `review.json` itself is read from disk because the held set is always
 * re-read when that file moves.
 */
export function loadedStamps(set: ReviewSet, stat: StatFile): Stamps {
  const assets: Record<string, string> = {};
  for (const asset of set.assets.entries()) assets[asset.path] = stamp(asset.mtimeMs, asset.size);
  return { set: stampOnDisk(set.path, stat), assets };
}

function stampOnDisk(path: string, stat: StatFile): string {
  const stats = stat(path);
  return stamp(stats?.mtimeMs ?? 0, stats?.size ?? 0);
}

export function stampsOf(set: ReviewSet, stat: StatFile): Stamps {
  const assets: Record<string, string> = {};
  for (const asset of set.assets.entries()) assets[asset.path] = stampOnDisk(asset.path, stat);
  return { set: stampOnDisk(set.path, stat), assets };
}
