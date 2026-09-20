/**
 * Where the pointer was last seen, in client coordinates.
 *
 * The colour readout has to answer "what is under the cursor" at moments when
 * the cursor has not moved — the mode was just turned on, the page scrolled, a
 * frame was stepped, a config was switched. In every one of those the *picture*
 * moves or changes under a stationary pointer, so a reading taken only on
 * `pointermove` is stale and silently describes the pixel that used to be there.
 *
 * The browser exposes no "current pointer position", so it must be remembered.
 * This is a **plain variable, not a signal**, and deliberately: nothing should
 * re-render because the pointer moved a pixel. It is written on every move and
 * read only when one of those triggers fires.
 *
 * `undefined` until the pointer is first seen — a page that was scrolled with
 * the keyboard and never touched has no answer, and inventing one would put a
 * readout on a pixel nobody is pointing at.
 */

export interface SeenPoint {
  readonly clientX: number;
  readonly clientY: number;
}

let seen: SeenPoint | undefined;

/** Start remembering. Returns the cleanup, for `onCleanup`. */
export function watchPointer(): () => void {
  const onMove = (event: PointerEvent) => {
    seen = { clientX: event.clientX, clientY: event.clientY };
  };
  // On the document, and always — the readout needs a position the moment `i`
  // is pressed, which is before any overlay exists to have heard a move.
  document.addEventListener("pointermove", onMove, { passive: true });
  // A pointer that has left the window is not over any picture, and a stale
  // position would keep a readout up after the cursor is gone.
  const onLeave = (event: PointerEvent) => {
    if (event.relatedTarget === null) seen = undefined;
  };
  document.addEventListener("pointerout", onLeave, { passive: true });
  return () => {
    document.removeEventListener("pointermove", onMove);
    document.removeEventListener("pointerout", onLeave);
  };
}

export function lastPointer(): SeenPoint | undefined {
  return seen;
}
