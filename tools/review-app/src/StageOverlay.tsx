import { For, Show, createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import { css } from "../styled-system/css";
import type { Rgb } from "./color";
import {
  type Box,
  type NormalRect,
  type Point,
  isDragWorthKeeping,
  rectFromDrag,
  rectToBox,
  samplePlan,
} from "./geometry";
import type { PointerMode } from "./keys";
import type { Patch } from "./patches";
import { lastPointer } from "./pointer";
import { sampleImage } from "./sample";

/**
 * The layer the pointer modes act on: patch rectangles and colour sampling.
 *
 * It lives **inside the stage**, sized and placed to the picture's painted box,
 * which is what makes it work in both zoom modes with no special cases. In `fit`
 * the box is the letterboxed picture rather than the `<img>` element, so a patch
 * cannot drift into the letterbox; in `fullsize` it is the natural-size picture
 * and it scrolls with it, because it is part of the scrolled content.
 *
 * Being exactly the painted box has a second payoff: a pointer position measured
 * against this element's own rect is already a position on the picture, so every
 * handler below works in one coordinate space and mid-scroll positions need no
 * correction.
 *
 * It is mounted only while a pointer mode is on. An always-mounted layer would
 * have to be `pointer-events: none`, and then turning a mode on would be a
 * property change on an element the browser has already decided is transparent
 * to hit-testing — cheaper to reason about is to not be there at all. It also
 * means patches are drawn only in patch mode, which is deliberate: the rest of
 * the time this page is for looking at a photograph.
 */

const styles = {
  layer: css.raw({
    position: "absolute",
    // Above the stacked renditions, below the pane's own overlays (the pan
    // controls and the "no rendition" note), which are rendered in the pane.
    zIndex: 2,
    touchAction: "none",
  }),
  patchMode: css.raw({ cursor: "crosshair" }),
  colorMode: css.raw({ cursor: "crosshair" }),

  /*
    A patch is drawn as an outline with a dark halo rather than a fill: it sits
    on a photograph whose colours are the thing under review, and a wash over one
    would tint exactly what someone is trying to judge. The halo is what keeps
    the outline visible over both a blown highlight and a black shadow — one
    colour cannot do that on its own.
  */
  patch: css.raw({
    position: "absolute",
    boxSizing: "border-box",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "patch.line",
    outlineWidth: "1px",
    outlineStyle: "solid",
    outlineColor: "patch.halo",
    outlineOffset: "1px",
    /*
      Hidden until wanted, by **opacity plus `pointer-events`** — not by
      `visibility`.

      Each half fixes a different bug. Bare `opacity: 0` leaves the button taking
      clicks, so every patch carried an invisible delete control at its corner;
      `pointer-events: none` is what closes that. But `visibility: hidden`, the
      obvious alternative, takes the button out of the tab order entirely — and
      since this is the *only* way to delete a patch and patches cannot be
      edited, a keyboard-only user could never correct one. An `opacity: 0`
      element is still focusable, so `:focus-within` brings it back.
    */
    "& [data-remove]": { opacity: 0, pointerEvents: "none" },
    "&:hover [data-remove]": { opacity: 1, pointerEvents: "auto" },
    "&:focus-within [data-remove]": { opacity: 1, pointerEvents: "auto" },
  }),
  pending: css.raw({ borderStyle: "dashed" }),
  /*
    **Inside the rectangle, always.** Drawn above it, the chip is clipped away by
    the scrolling viewport whenever the patch reaches the top of it — which in
    `fullsize` is *any* patch, because scrolling brings each one to the top in
    turn. A flip keyed on the patch's position within the image only caught the
    ones near the image's own top edge; catching the rest would mean keying a
    style on `scrollTop`, i.e. coupling the chip to scrolling, which this app
    keeps out of the paint path. Inside is unconditional and cannot be clipped.
    It costs the rectangle's top-left corner, which is free: patches are drawn
    only in patch mode, never while a picture is being judged.

    It is **not bounded by the rectangle**, though. A patch is often smaller than
    its label — a 40px square marked "white shirt" — and a chip clipped to the
    rectangle truncates to a letter and an ellipsis, which names nothing. It runs
    past the edge instead; two chips may overlap, and reading the label beats
    avoiding that.
  */
  chip: css.raw({
    position: "absolute",
    top: "0",
    insetInlineStart: "0",
    paddingBlock: "0",
    paddingInline: "4px",
    backgroundColor: "patch.line",
    color: "patch.ink",
    fontSize: "key",
    lineHeight: "snug",
    whiteSpace: "nowrap",
    pointerEvents: "none",
  }),
  // Outside the rectangle's top-right corner, so it never covers the pixels the
  // patch was drawn around.
  remove: css.raw({
    position: "absolute",
    top: "0",
    insetInlineEnd: "0",
    transform: "translate(50%, -50%)",
    width: "patchClose",
    height: "patchClose",
    display: "grid",
    alignItems: "center",
    justifyItems: "center",
    padding: "0",
    borderRadius: "round",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "patch.line",
    backgroundColor: "scrim",
    color: "fg",
    fontFamily: "body",
    fontSize: "key",
    lineHeight: "flush",
    cursor: "pointer",
  }),
  marquee: css.raw({
    position: "absolute",
    boxSizing: "border-box",
    borderWidth: "1px",
    borderStyle: "dashed",
    borderColor: "patch.line",
    backgroundColor: "patch.wash",
    pointerEvents: "none",
  }),
};

/** A reading handed back to the frame, in client coordinates. */
export interface ColorSample {
  readonly clientX: number;
  readonly clientY: number;
  readonly rgb: Rgb;
}

interface Props {
  mode: Exclude<PointerMode, "off">;
  /** The picture's painted box, relative to the stage. */
  painted: Box;
  /** The rendition being shown, which is what a colour is read from. */
  image: HTMLImageElement | undefined;
  /**
   * Changes when a rendition finishes loading.
   *
   * A config switched to before its picture has decoded has nothing to sample,
   * so the reading has to be taken again once it does — otherwise the readout
   * stays blank until the pointer happens to move.
   */
  loadedTick: number;
  patches: readonly Patch[];
  /** The rectangle waiting for a label, drawn while its dialog is up. */
  pendingPatch: NormalRect | undefined;
  onDraw: (rect: NormalRect) => void;
  onRemove: (patchId: string) => void;
  onSample: (sample: ColorSample | undefined) => void;
  /** A click in colour mode: the reading under the cursor was asked for. */
  onPick: () => void;
}

export function StageOverlay(props: Props) {
  let layer: HTMLDivElement | undefined;
  const [drag, setDrag] = createSignal<{ a: Point; b: Point } | undefined>();

  /** The painted box in this element's own coordinates — which is its own size. */
  const local = (): Box => ({
    x: 0,
    y: 0,
    width: props.painted.width,
    height: props.painted.height,
  });

  const pointAt = (event: PointerEvent): Point => {
    const box = layer?.getBoundingClientRect();
    return { x: event.clientX - (box?.left ?? 0), y: event.clientY - (box?.top ?? 0) };
  };

  /**
   * Read the colour at a client point, or clear the readout.
   *
   * **`elementFromPoint` decides whether this overlay owns the point**, rather
   * than a containment test against its own rect. Two reasons, both real here:
   * in `fullsize` the overlay is far larger than the pane that clips it, so a
   * rect test would have a frame claim a pointer that is actually over its
   * neighbour — and several frames are mounted at once, so more than one would
   * claim it. Hit-testing answers the question the pointer itself would.
   */
  const sampleAt = (point: { clientX: number; clientY: number } | undefined) => {
    const image = props.image;
    if (!point || !image || !layer) return props.onSample(undefined);
    const over = document.elementFromPoint(point.clientX, point.clientY);
    if (!over || !layer.contains(over)) return props.onSample(undefined);
    const box = layer.getBoundingClientRect();
    const at = { x: point.clientX - box.left, y: point.clientY - box.top };
    const plan = samplePlan(
      at,
      local(),
      image.naturalWidth,
      image.naturalHeight,
      window.devicePixelRatio,
    );
    if (!plan) return props.onSample(undefined);
    const rgb = sampleImage(image, plan.x, plan.y, plan.span);
    props.onSample(rgb ? { clientX: point.clientX, clientY: point.clientY, rgb } : undefined);
  };

  /*
    Read again without a pointer event, from the last place the pointer was seen.

    Everything below moves the picture under a stationary cursor, and each one
    used to leave the readout describing a pixel that had moved away: turning the
    mode on at all, scrolling the page, stepping a frame with `j`/`k` (which
    scrolls), panning inside a `fullsize` picture, and switching config — where
    the pixel does not even move, but what is under it is a different rendering,
    which is the whole comparison.
  */
  const resample = () => {
    if (props.mode !== "color") return;
    sampleAt(lastPointer());
  };

  // **However the layer goes away, the reading goes with it.** It unmounts when
  // the mode is turned off, when the frame scrolls out of the mount window, and
  // when a live refresh removes the rendition under it — and the readout is
  // rendered by the *pane*, not by this element, so without this it would go on
  // describing a picture that is no longer on screen.
  onCleanup(() => props.onSample(undefined));

  onMount(() => {
    // The mode was just turned on, with the pointer already over a picture.
    resample();
    // `capture`, so one listener catches the page scrolling *and* the pane
    // scrolling inside a `fullsize` picture — scroll events do not bubble.
    //
    // Called straight from the handler, like `paintMap` in `ImageSection` and
    // for the same reason: the browser already coalesces scroll to about one
    // per frame, and unlike `requestAnimationFrame` this still runs in a
    // backgrounded tab. It cannot feed itself either — the readout is absolutely
    // positioned and changes no layout, so it cannot move what it just measured.
    const onScroll = () => resample();
    window.addEventListener("scroll", onScroll, true);
    onCleanup(() => window.removeEventListener("scroll", onScroll, true));
  });

  /*
    The config changed (a different `<img>` under the same pixel), the picture was
    re-laid-out, or a rendition finished decoding.

    **`props.mode` is in here, and `onMount` is not enough on its own.** The
    overlay is mounted for *either* pointer mode, and `ImageSection` shows it with
    a plain `Show` whose condition stays truthy across patch -> colour — so the
    component is never recreated and `onMount` never runs again, while
    `ImageSection` has meanwhile cleared the reading. Without this dependency,
    switching straight from patch mode to colour mode left the readout blank until
    the pointer was jogged; `off -> colour` worked, which is exactly why it
    survived testing.
  */
  createEffect(
    on(
      [() => props.mode, () => props.image, () => props.painted, () => props.loadedTick],
      resample,
      { defer: true },
    ),
  );

  const onPointerDown = (event: PointerEvent & { currentTarget: HTMLDivElement }) => {
    // **A tap has no move before it.** Touch and non-hovering pens emit
    // pointerdown/up/click with no `pointermove` at all, so nothing would have
    // sampled by the time the click asks to copy, and colour mode would appear
    // dead. Reading here makes the click self-sufficient on any pointer. (A
    // hovering readout is still a mouse affordance — there is no hover to
    // follow on a touchscreen — but a tap should at least answer.)
    if (props.mode === "color") return sampleAt(event);
    if (props.mode !== "patch" || event.button !== 0) return;
    // Captured so a drag that runs off the picture — off the *window*, even —
    // still ends on this element. Without it, releasing outside leaves the
    // marquee drawn and the next click finishing a rectangle nobody is drawing.
    //
    // It may refuse: capture requires the pointer to still be active, and a
    // `NotFoundError` here would otherwise take the whole gesture down with it.
    // Losing capture costs a drag that ends off the picture, not the feature.
    try {
      event.currentTarget.setPointerCapture(event.pointerId);
    } catch {
      /* the drag still works; it just will not follow the pointer off the element */
    }
    event.preventDefault();
    const at = pointAt(event);
    setDrag({ a: at, b: at });
  };

  const onPointerMove = (event: PointerEvent) => {
    if (props.mode === "color") return sampleAt(event);
    const started = drag();
    if (!started) return;
    setDrag({ a: started.a, b: pointAt(event) });
  };

  const onPointerUp = (event: PointerEvent) => {
    const started = drag();
    setDrag(undefined);
    if (props.mode !== "patch" || !started) return;
    const at = pointAt(event);
    // A click is a miss, not a request for an empty rectangle.
    if (!isDragWorthKeeping(started.a, at, local())) return;
    props.onDraw(rectFromDrag(started.a, at, local()));
  };

  const marquee = () => {
    const started = drag();
    return started ? rectToBox(rectFromDrag(started.a, started.b, local()), local()) : undefined;
  };

  const place = (box: Box) => ({
    left: `${box.x}px`,
    top: `${box.y}px`,
    width: `${box.width}px`,
    height: `${box.height}px`,
  });

  return (
    <div
      ref={(node) => (layer = node)}
      class={css(styles.layer, props.mode === "patch" ? styles.patchMode : styles.colorMode)}
      style={place(props.painted)}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => setDrag(undefined)}
      onClick={() => {
        if (props.mode === "color") props.onPick();
      }}
      // Leaving the picture clears the readout rather than freezing the last
      // value beside a cursor that is somewhere else entirely.
      onPointerLeave={() => {
        if (props.mode === "color") props.onSample(undefined);
      }}
    >
      <Show when={props.mode === "patch"}>
        <For each={props.patches}>
          {(patch) => (
            <div class={css(styles.patch)} style={place(rectToBox(patch, local()))}>
              <span class={css(styles.chip)} title={patch.label}>
                {patch.label}
              </span>
              <button
                type="button"
                data-remove
                class={css(styles.remove)}
                aria-label={`Delete patch ${patch.label}`}
                title={`Delete patch ${patch.label}`}
                // The layer owns pointer-down for drawing; without this, removing
                // a patch also starts a marquee under the cursor.
                onPointerDown={(event) => event.stopPropagation()}
                onClick={() => props.onRemove(patch.id)}
              >
                ×
              </button>
            </div>
          )}
        </For>
        <Show when={props.pendingPatch}>
          {(rect) => (
            <div
              class={css(styles.patch, styles.pending)}
              style={place(rectToBox(rect(), local()))}
            />
          )}
        </Show>
      </Show>
      <Show when={marquee()}>
        {(box) => <div class={css(styles.marquee)} style={place(box())} />}
      </Show>
    </div>
  );
}
