/**
 * Keyboard mapping, kept free of the DOM so it can be tested directly.
 *
 * `1`-`9` select the first nine configs and `0` selects the tenth — the layout
 * of the number row, so the keys read left to right exactly like the buttons.
 * Beyond ten there is no key; the buttons still work.
 *
 * `j`/`k` move between frames and `h`/`l` step the selected config, in the vi
 * directions rather than the arrow keys': the arrows already scroll the page,
 * and a frame is a whole screen, so binding them would take away ordinary
 * scrolling inside a `fullsize` picture.
 *
 * `p` and `i` are the two pointer modes, and pressing the one already on turns
 * it off — see `nextPointerMode`.
 */

/** How many configs the number row can reach. */
export const KEYBOARD_REACHABLE_CONFIGS = 10;

export type KeyAction =
  | { readonly kind: "config"; readonly index: number }
  | { readonly kind: "zoom" }
  /** Move the current frame: `j` down the set, `k` up it. */
  | { readonly kind: "frame"; readonly delta: 1 | -1 }
  /** Step the selected config: `l` forward, `h` back. Wraps. */
  | { readonly kind: "configStep"; readonly delta: 1 | -1 }
  /** Note on the current frame (`a`), every note (`n`), clear them all (`c`). */
  | { readonly kind: "note" }
  | { readonly kind: "notes" }
  | { readonly kind: "clearNotes" }
  /** Show or hide the charts row (`m`), giving its band back to the picture. */
  | { readonly kind: "metrics" }
  /**
   * Enter or leave a pointer mode: `p` draws patches, `i` reads colour.
   *
   * One action for both, carrying which mode, because the two are **mutually
   * exclusive** — each takes over the pointer on the picture, and a pointer that
   * did two things at once would do neither predictably. Modelling them as one
   * selection rather than two booleans is what makes an illegal pair
   * unrepresentable, the same reason `FilmBaseSource` is one enum in nc.
   */
  | { readonly kind: "pointerMode"; readonly mode: PointerMode };

/**
 * What the pointer does over a picture.
 *
 * `"off"` is the ordinary page: the pointer selects text, scrolls, and clicks
 * the controls. The other two claim it.
 */
export type PointerMode = "off" | "patch" | "color";

/**
 * The action a keypress should perform, or `null` for keys we leave alone.
 *
 * Modified keypresses are always `null`: `⌘1` switches browser tabs and `⌃1` is
 * a window-manager binding on some setups, so claiming them would break the
 * user's own shortcuts.
 */
export function actionForKey(
  key: string,
  modifiers: { alt?: boolean; ctrl?: boolean; meta?: boolean },
  configCount: number,
): KeyAction | null {
  if (modifiers.alt || modifiers.ctrl || modifiers.meta) return null;

  if (key === "f" || key === "F") return { kind: "zoom" };
  if (key === "j" || key === "J") return { kind: "frame", delta: 1 };
  if (key === "k" || key === "K") return { kind: "frame", delta: -1 };
  if (key === "l" || key === "L") return { kind: "configStep", delta: 1 };
  if (key === "h" || key === "H") return { kind: "configStep", delta: -1 };
  if (key === "a" || key === "A") return { kind: "note" };
  if (key === "n" || key === "N") return { kind: "notes" };
  if (key === "c" || key === "C") return { kind: "clearNotes" };
  if (key === "m" || key === "M") return { kind: "metrics" };
  if (key === "p" || key === "P") return { kind: "pointerMode", mode: "patch" };
  if (key === "i" || key === "I") return { kind: "pointerMode", mode: "color" };

  if (key.length === 1 && key >= "0" && key <= "9") {
    // '1'..'9' are 0..8; '0' is the tenth slot rather than the first.
    const index = key === "0" ? 9 : key.charCodeAt(0) - "1".charCodeAt(0);
    return index < configCount ? { kind: "config", index } : null;
  }

  return null;
}

/**
 * The config `h`/`l` should land on, **wrapping** at both ends.
 *
 * Wrapping where `j`/`k` clamp, and the difference is the thing being stepped
 * through rather than an inconsistency: the configs are a handful of renderings
 * of one frame, cycled repeatedly to see what moves, so running off the end and
 * back on is the gesture. Frames are a long list you are working down, where
 * wrapping from the last to the first would lose your place.
 */
export function stepConfigIndex(index: number, delta: 1 | -1, count: number): number {
  if (count <= 0) return 0;
  return (((index + delta) % count) + count) % count;
}

/** The key that selects a config, or `undefined` past the number row. */
export function keyForConfigIndex(index: number): string | undefined {
  if (index < 0 || index >= KEYBOARD_REACHABLE_CONFIGS) return undefined;
  return index === 9 ? "0" : String(index + 1);
}

/** Typing in a field must not be swallowed as a shortcut. */
export function isTextEntry(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}

/**
 * The mode `p` or `i` lands on: the one asked for, or `"off"` if it was already on.
 *
 * So each key is its own toggle while the pair stays exclusive — `p` then `i`
 * leaves you in colour mode rather than in both, and a second `p` leaves the
 * picture alone. Pressing the other mode's key is a switch, not an error: having
 * to turn one off before turning the other on would be a rule with nothing
 * behind it.
 */
export function nextPointerMode(current: PointerMode, asked: PointerMode): PointerMode {
  return current === asked ? "off" : asked;
}
