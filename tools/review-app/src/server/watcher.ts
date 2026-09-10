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
import { diffStamps, hasChange, stampsOf, watchTargets, type Stamps } from "./stamps";
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
}

const GLOBAL_KEY = "__ncReviewWatch";
const store = globalThis as typeof globalThis & { [GLOBAL_KEY]?: WatchState };

const mtime = (path: string): number | undefined => {
  try {
    return statSync(path).mtimeMs;
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
  const state = await ensureWatching();
  state.listeners.add(listener);
  return () => state.listeners.delete(listener);
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
    stamps: stampsOf(set, mtime),
    timer: undefined,
    key: set.path,
  };
  store[GLOBAL_KEY] = state;

  const targets = watchTargets(
    set.dir,
    set.assets.entries().map((asset) => asset.path),
  );
  const onEvent = () => {
    clearTimeout(state.timer);
    state.timer = setTimeout(() => void settle(state), SETTLE_MS);
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

  return state;
}

async function settle(state: WatchState): Promise<void> {
  // Re-read only after the model may have moved: a changed `review.json` can
  // rename or add renditions, so the stamps must be taken from the new set.
  const before = state.stamps;
  const setChangedOnDisk = mtime(state.key) !== before.set;
  if (setChangedOnDisk) invalidateReviewSet();

  let set;
  try {
    set = await currentReviewSet();
  } catch {
    // A half-written or broken `review.json`. Say nothing and wait for the next
    // event — the editor is probably still saving.
    return;
  }

  const after = stampsOf(set, mtime);
  state.stamps = after;
  if (!hasChange(diffStamps(before, after))) return;
  for (const listener of state.listeners) listener();
}

function stopWatching(state: WatchState): void {
  clearTimeout(state.timer);
  for (const watcher of state.watchers) watcher.close();
  state.watchers = [];
}
