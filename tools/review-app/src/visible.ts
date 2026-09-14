/**
 * Which frames are mounted, given which ones the viewport can see.
 *
 * A set may hold dozens of frames x several configs of full-size JPEGs, and
 * mounting all of them means fetching all of them. So only the frames near the
 * viewport render their pictures; the rest are empty shells of exactly the same
 * height.
 *
 * The decision is split out here because nothing in a `.tsx` file is tested —
 * `vite.config.ts` collects only `.test.ts` under `src/`, with no DOM — so the
 * component keeps the observer and this keeps the arithmetic.
 *
 * **Overscan is counted in frames, not in pixels.** The observer is asked for
 * plain intersection (no `rootMargin`), which answers "what is on screen" and
 * nothing else; how far beyond that to reach is decided here, where it can be
 * read and changed. Going the other way — a `rootMargin` of one viewport — would
 * fold the two questions into one number and leave neither observable.
 */

/**
 * How many frames beyond the visible ones stay mounted, on each side.
 *
 * One is enough to make scrolling seamless: a frame is a whole viewport tall, so
 * its neighbour is at most one flick away and is already loaded when it arrives.
 */
export const OVERSCAN = 1;

/**
 * The ids to render, widest visible span expanded by `overscan` and clamped.
 *
 * Ids in `visible` that are not in `ids` are ignored rather than refused: a live
 * edit to `review.json` can drop an image between an observer callback and this
 * call, and a stale id must not decide the window — or empty it.
 *
 * With nothing visible the head of the list is mounted. That is the state before
 * the first observer callback, on a page that has not been scrolled, so the
 * alternative is a first paint with every frame blank.
 */
export function mountWindow(
  ids: readonly string[],
  visible: ReadonlySet<string>,
  overscan: number = OVERSCAN,
): Set<string> {
  if (ids.length === 0) return new Set();

  let first = Infinity;
  let last = -Infinity;
  for (const [index, id] of ids.entries()) {
    if (!visible.has(id)) continue;
    if (index < first) first = index;
    if (index > last) last = index;
  }

  // Nothing on screen (or nothing recognised): mount from the top, the same span
  // a visible first frame would have produced.
  if (last < 0) {
    first = 0;
    last = 0;
  }

  const from = Math.max(0, first - overscan);
  const to = Math.min(ids.length - 1, last + overscan);
  return new Set(ids.slice(from, to + 1));
}

/** Set equality, so an observer callback reporting no change re-renders nothing. */
export function sameIds(a: ReadonlySet<string>, b: ReadonlySet<string>): boolean {
  if (a.size !== b.size) return false;
  for (const id of a) if (!b.has(id)) return false;
  return true;
}

/**
 * The frame the viewer is looking at: the **lowest** one whose picture is on
 * screen.
 *
 * Lowest rather than topmost because scrolling down reveals the next picture at
 * the bottom of the screen, and that is the frame you are moving *to* — so it
 * takes the mark as soon as it appears, and the one you are leaving gives it up.
 *
 * Between frames no picture need be on screen at all (a frame's charts and the
 * next one's head can fill the viewport between them), so the previous answer is
 * kept rather than blanking the mark and flickering it back. `previous` is only
 * honoured while it is still in the set; a live edit can remove it.
 */
export function currentFrame(
  ids: readonly string[],
  picturesOnScreen: ReadonlySet<string>,
  previous: string | undefined,
): string | undefined {
  if (ids.length === 0) return undefined;
  for (let i = ids.length - 1; i >= 0; i -= 1) {
    const id = ids[i];
    if (id !== undefined && picturesOnScreen.has(id)) return id;
  }
  return previous !== undefined && ids.includes(previous) ? previous : ids[0];
}

/**
 * Where `j`/`k` should take the viewer, or `undefined` to stay put.
 *
 * **An unaligned frame is aligned first, in either direction.** Half a frame on
 * screen is the common state after a free scroll, and from there the first press
 * settles what you are already looking at rather than skipping it. The rule is
 * stated for `j`; it is applied to `k` as well so that neither key can carry you
 * past a frame you never saw squarely — the alternative, `k` always stepping
 * back, means a small scroll down from an aligned frame costs you that frame on
 * the way back up.
 *
 * Past either end nothing happens: the set does not wrap, because `j` held down
 * at the last frame jumping to the first is never what was meant.
 */
export function frameForKey(
  ids: readonly string[],
  current: string | undefined,
  delta: 1 | -1,
  currentIsAligned: boolean,
): string | undefined {
  if (ids.length === 0) return undefined;
  const index = current === undefined ? -1 : ids.indexOf(current);
  // Nothing current, or a stale id: start at the top of the set.
  if (index === -1) return ids[0];
  if (!currentIsAligned) return ids[index];
  return ids[index + delta];
}
