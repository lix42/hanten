/**
 * Watch the review set and tell connected pages when it changes.
 *
 * The point is that re-running `nc` over a set updates the page: a rewritten
 * rendition changes its mtime, which changes its `/img/` URL, which is what
 * makes the browser fetch the new picture instead of the one it has cached.
 *
 * Held on `globalThis` because the dev server re-executes module graphs on
 * change; a module-level watcher would be recreated on every edit and the old
 * ones would go on firing.
 */

import { watch, type FSWatcher } from "node:fs";
import { statSync } from "node:fs";
import {
  diffStamps,
  hasChange,
  loadedStamps,
  stampsOf,
  watchTargets,
  watchTargetsKey,
  type Stamps,
} from "./stamps";
import type { ReviewSet } from "./reviewSet";
import { currentReviewSet, invalidateReviewSet } from "./state";

/** Coalesces the burst of events one save produces. */
const SETTLE_MS = 120;

type Listener = () => void;

interface WatchState {
  watchers: FSWatcher[];
  listeners: Set<Listener>;
  stamps: Stamps;
  timer: NodeJS.Timeout | undefined;
  key: string;
  /** Identity of the directories currently watched, so a move is noticed. */
  targets: string;
}

const GLOBAL_KEY = "__ncReviewWatch";
const store = globalThis as typeof globalThis & { [GLOBAL_KEY]?: WatchState };

const statFile = (path: string): { mtimeMs: number; size: number } | undefined => {
  try {
    const stats = statSync(path);
    return { mtimeMs: stats.mtimeMs, size: stats.size };
  } catch {
    return undefined;
  }
};

/**
 * Subscribe to set changes, starting the watch if this is the first listener.
 *
 * Returns the unsubscribe. The watch is left running when the last listener
 * goes: a page being reloaded drops and re-adds one constantly, and tearing the
 * watchers down and back up each time buys nothing in a dev server.
 */
export async function onReviewSetChange(listener: Listener): Promise<() => void> {
  // `ensureWatching` reads and writes the global across an `await`, so two
  // connections arriving together could each install a `WatchState` and orphan
  // the first one's watchers. One start at a time.
  starting ??= ensureWatching().finally(() => {
    starting = undefined;
  });
  const state = await starting;
  state.listeners.add(listener);
  return () => state.listeners.delete(listener);
}

let starting: Promise<WatchState> | undefined;

function scheduleSettle(state: WatchState): void {
  clearTimeout(state.timer);
  state.timer = setTimeout(() => void settle(state), SETTLE_MS);
}

/**
 * Point the watchers at the directories this set actually occupies.
 *
 * Re-run whenever the set is re-read: editing `review.json` can move a rendition
 * into a directory nothing was watching, and watchers derived from the previous
 * asset map would report nothing that happened there.
 */
function installWatchers(state: WatchState, set: ReviewSet): void {
  const targets = watchTargets(
    set.dir,
    set.assets.entries().map((asset) => asset.path),
  );
  const key = watchTargetsKey(targets);
  if (state.watchers.length > 0 && key === state.targets) return;

  for (const watcher of state.watchers) watcher.close();
  state.watchers = [];
  state.targets = key;

  const onEvent = () => {
    scheduleSettle(state);
  };
  for (const [dir, recursive] of [
    [targets.recursive, true] as const,
    ...targets.others.map((dir) => [dir, false] as const),
  ]) {
    try {
      const watcher = watch(dir, { recursive, persistent: false }, onEvent);
      // A watcher that errors (the directory was removed) must not take the
      // server down; the page simply stops updating for that directory.
      watcher.on("error", () => undefined);
      state.watchers.push(watcher);
    } catch {
      // Unwatchable directory — same reasoning.
    }
  }
}

async function ensureWatching(): Promise<WatchState> {
  const set = await currentReviewSet();
  const existing = store[GLOBAL_KEY];
  if (existing && existing.key === set.path) return existing;

  // The set moved (a restart with a different REVIEW_SET behind the same
  // process): drop the old watchers before installing new ones.
  if (existing) stopWatching(existing);

  const state: WatchState = {
    watchers: [],
    listeners: existing?.listeners ?? new Set(),
    // Baseline is what the *loaded set* carries, not what is on disk now: a
    // rendition rewritten between the page's first read and this moment would
    // otherwise be recorded as already-seen and never reported.
    stamps: loadedStamps(set, statFile),
    timer: undefined,
    key: set.path,
    targets: "",
  };
  store[GLOBAL_KEY] = state;
  installWatchers(state, set);
  // Settle once immediately, so any drift against that baseline is caught now
  // rather than waiting for the next unrelated filesystem event.
  scheduleSettle(state);

  return state;
}

async function settle(state: WatchState): Promise<void> {
  const before = state.stamps;

  let set;
  try {
    set = await currentReviewSet();
  } catch {
    // A half-written or broken `review.json`. Say nothing and wait for the next
    // event — the editor is probably still saving.
    return;
  }

  // Stamps are taken from disk, so they see a re-rendered file even while the
  // held set still carries its old mtime.
  if (!hasChange(diffStamps(before, stampsOf(set, statFile)))) return;

  // Something moved, so the held set is stale — its asset map froze every mtime
  // at parse time, and those mtimes *are* the `/img/` URLs. Dropping it only
  // when `review.json` itself changed left a re-rendered frame addressed by its
  // old URL, which the browser then served from an `immutable` cache entry: the
  // page showed the previous render with nothing reporting a problem.
  invalidateReviewSet();
  try {
    set = await currentReviewSet();
  } catch {
    return;
  }

  // The re-read set may occupy different directories than the watch covers.
  installWatchers(state, set);
  state.stamps = stampsOf(set, statFile);
  for (const listener of state.listeners) listener();
}

function stopWatching(state: WatchState): void {
  clearTimeout(state.timer);
  for (const watcher of state.watchers) watcher.close();
  state.watchers = [];
}
