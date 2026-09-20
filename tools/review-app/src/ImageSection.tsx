import { For, Show, createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import { css } from "../styled-system/css";
import { domId } from "./charts/domId";
import { MetricsPanel } from "./charts/MetricsPanel";
import { ColorReadout } from "./ColorReadout";
import { type Rgb, toHex } from "./color";
import { type Box, type NormalRect, type Point, paintedBox } from "./geometry";
import type { PointerMode } from "./keys";
import type { Patch } from "./patches";
import { type ColorSample, StageOverlay } from "./StageOverlay";
import type { Rendition, ReviewConfig, ReviewImage, ZoomMode } from "./review";

// Border longhands rather than the `border` shorthand: `previewActive`
// overrides only the colour, and `css()` merges by property — a base that
// spelled the whole border as one shorthand would leave the override as a
// second, competing declaration. The grid-placement longhands below are not
// that: nothing overrides them, and `gridArea: "1 / 1"` would work fine now.
// They are inherited spelling from StyleX, which dropped `gridArea` silently.
const styles = {
  // Fills the one-screen shell `App` renders around it. Head and strip take what
  // they need, the charts take at most their band, and the picture gets every
  // pixel left over — so the thing this app exists to show is what grows when
  // there is room and what is measured against when there is not.
  section: css.raw({
    display: "flex",
    flexDirection: "column",
    height: "full",
    minHeight: "zero",
    paddingBlock: "20px",
    paddingInline: "16px",
    boxSizing: "border-box",
  }),
  head: css.raw({
    display: "flex",
    gap: "12px",
    alignItems: "baseline",
    marginBottom: "8px",
    flexShrink: 0,
  }),
  label: css.raw({ fontWeight: "semibold" }),
  // The mark on the frame you are looking at. Underline rather than a border or
  // a background: text decoration takes no space, so marking a frame cannot
  // change its head's height — and a frame's height is the invariant the whole
  // page rests on.
  labelCurrent: css.raw({
    textDecorationLine: "underline",
    textDecorationColor: "accent",
    textDecorationThickness: "2px",
    textUnderlineOffset: "5px",
  }),
  note: css.raw({ color: "accent", fontVariantNumeric: "tabular-nums" }),
  noted: css.raw({ color: "accent", fontSize: "meta" }),
  patched: css.raw({ color: "patch.line", fontSize: "meta" }),

  // Scrolls sideways rather than wrapping. A wrapping strip makes the head
  // taller on sets with many configs, which would come straight out of the
  // picture's height — and by a different amount per set.
  strip: css.raw({
    display: "flex",
    flexWrap: "nowrap",
    gap: "8px",
    alignItems: "center",
    marginBottom: "10px",
    flexShrink: 0,
    overflowX: "auto",
    overflowY: "hidden",
  }),
  preview: css.raw({
    padding: "0",
    flexShrink: 0,
    borderRadius: "md",
    borderWidth: "2px",
    borderStyle: "solid",
    borderColor: "transparent",
    backgroundColor: "transparent",
    cursor: "pointer",
    lineHeight: "flush",
  }),
  previewActive: css.raw({ borderColor: "accent" }),
  previewImage: css.raw({
    width: "thumbWidth",
    height: "thumbHeight",
    objectFit: "cover",
    borderRadius: "sm",
    display: "block",
  }),
  previewMissing: css.raw({
    width: "thumbWidth",
    height: "thumbHeight",
    borderRadius: "sm",
    backgroundColor: "missing",
    color: "fg.dim",
    fontSize: "key",
    display: "grid",
    alignItems: "center",
    justifyItems: "center",
    lineHeight: "snug",
  }),

  // The mini-map: where the fullsize viewport currently sits inside the image.
  mapOuter: css.raw({
    marginInlineStart: "auto",
    flexShrink: 0,
    width: "thumbWidth",
    height: "thumbHeight",
    borderRadius: "sm",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    backgroundColor: "panel",
    position: "relative",
    overflow: "hidden",
  }),
  mapWindow: css.raw({
    position: "absolute",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "accent",
    backgroundColor: "accent.wash",
  }),

  // The picture absorbs whatever the head and the charts leave, down to a floor.
  // Both this and the band shrink (`flex-shrink: 1` on each); what makes the
  // *charts* the side that gives way is the asymmetry in their floors — this one
  // stops at `pictureFloor`, the band's `minHeight` is zero — so once the picture
  // is at its floor every further pixel of deficit comes out of the band.
  frame: css.raw({
    position: "relative",
    flexGrow: 1,
    // **`flexBasis: zero` is load-bearing.** At the default `auto` the picture's
    // base size is its own content height, so it does not grow into free space —
    // it starts oversized and pushes the charts out of the frame entirely. From
    // zero it takes exactly what the head and the charts leave, and `minHeight`
    // is then what it is guaranteed.
    flexBasis: "zero",
    minHeight: "pictureFloor",
  }),
  viewport: css.raw({
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    borderRadius: "lg",
    backgroundColor: "panel",
    overflow: "auto",
    height: "full",
    boxSizing: "border-box",
  }),
  // Every rendition occupies the *same* grid cell, so switching config cannot
  // move the picture by a pixel — the whole point of comparing this way.
  // Inactive ones stay laid out (hidden, not removed) so nothing reflows.
  // `height: full` gives the stage the viewport's own (definite) height. How the
  // renditions sit inside it differs by mode — see `stageFit` / `stageFullsize`.
  // `position: relative` is the overlay's anchor: `StageOverlay` is placed at
  // the picture's painted box in *this* element's coordinates, so it scrolls with
  // the picture in `fullsize` and letterboxes with it in `fit`.
  stage: css.raw({ display: "grid", height: "full", position: "relative" }),
  // **One cell, exactly the viewport, with the renditions stretched into it.**
  // The explicit `100%` tracks are what make the cell's height *definite*: a
  // percentage height on a grid item resolves against its grid area, and an
  // implicit row is content-sized, so `height`/`max-height` in percent resolve
  // against an indefinite value and are dropped. That is precisely how `fit`
  // stopped constraining height — it had been `maxHeight: stageCap` (82vh, an
  // absolute length, which always resolved) and became `maxHeight: full`, which
  // silently did nothing: the picture was width-limited only, overflowed the
  // viewport, and put a second scrollbar inside the page's own.
  stageFit: css.raw({
    gridTemplateRows: "100%",
    gridTemplateColumns: "100%",
    alignItems: "stretch",
    justifyItems: "stretch",
  }),
  // Natural size, top-left, overflowing the stage so the viewport scrolls.
  stageFullsize: css.raw({ alignItems: "start", justifyItems: "start" }),
  rendition: css.raw({ gridRowStart: "1", gridColumnStart: "1", display: "block" }),
  hidden: css.raw({ visibility: "hidden" }),
  // Fills the cell and is *contained* in it, so the whole frame is visible at
  // once whatever its aspect — which is what `fit` should have meant all along.
  // `object-fit` scales by the decoded image's own intrinsic ratio, not by the
  // `width=`/`height=` attributes, so a stale dimension in `review.json` still
  // cannot distort the picture — the same property `fullsize` protects below.
  fit: css.raw({
    width: "full",
    height: "full",
    objectFit: "contain",
    objectPosition: "center",
  }),
  // `width`/`height` set to `natural` (i.e. `auto`), not just the max-* releases
  // set to `unconstrained` (i.e. `none`). The `width=`/`height=`
  // attributes are presentational, so with nothing overriding them they *become*
  // the rendered size — a stale or copied dimension in `review.json` would then
  // silently scale the picture in the one mode whose purpose is 1:1 inspection,
  // and two renditions declaring different dimensions would render at different
  // sizes in the same grid cell, moving the picture on toggle. `fullsize` means
  // natural size; say so.
  fullsize: css.raw({
    maxWidth: "unconstrained",
    maxHeight: "unconstrained",
    width: "natural",
    height: "natural",
  }),

  pan: css.raw({
    position: "absolute",
    // **Above `StageOverlay`.** Neither the pane, the viewport nor the stage
    // creates a stacking context (all are `position: relative` at `z-index:
    // auto`), so the overlay's own `z-index` competes with these controls
    // directly — and being image-sized it covers them. Left equal, a click on a
    // visible pan arrow in either pointer mode sampled a colour or started a
    // patch instead of panning, which is the only way to move around a
    // `fullsize` picture.
    zIndex: 3,
    display: "grid",
    alignItems: "center",
    justifyItems: "center",
    width: "panControl",
    height: "panControl",
    borderRadius: "lg",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    backgroundColor: "scrim",
    color: "fg",
    fontSize: "glyph",
    cursor: "pointer",
    padding: "0",
  }),
  panUp: css.raw({ top: "8px", insetInlineStart: "half" }),
  panDown: css.raw({ bottom: "8px", insetInlineStart: "half" }),
  panLeft: css.raw({ insetInlineStart: "8px", top: "half" }),
  panRight: css.raw({ insetInlineEnd: "8px", top: "half" }),
  // The charts' share of the screen. The height lives here rather than inside
  // `MetricsPanel` because that component is also mounted on `/charts`, in
  // ordinary flow, where it should draw at natural size.
  //
  // `flexShrink: 1` with a zero floor is what happens below about 845px of
  // viewport: there is not room for this band *and* `pictureFloor`, and
  // `minHeight: zero` against the picture's floor is what decides who yields.
  // Both shrink; only this one may shrink to nothing. The charts are a summary
  // of numbers held elsewhere, whereas the picture is the thing this app exists
  // to show.
  band: css.raw({ flexShrink: 1, minHeight: "zero", display: "flex" }),
  // **The stated height applies only when charts are actually drawn.** A set
  // rendered with `--no-metrics` has nothing to put in the band, and holding
  // 280px open for one line of "not measured" took ~220px of picture from every
  // frame — measured on a 1618px viewport, band 280 against 59 of content.
  bandCharted: css.raw({ height: "chartsBand" }),
  // An overlay, not a replacement. Swapping the scroller out for a message
  // unmounts it, and the scroll position goes with it: park somewhere in
  // fullsize, toggle through a config that has no rendition, and you come back
  // to the top-left. Keeping the stage mounted (every rendition hidden) holds
  // both the box and the position.
  missing: css.raw({
    position: "absolute",
    // Above the pointer layer for the same reason as the pan controls.
    zIndex: 3,
    top: "12px",
    insetInlineStart: "half",
    paddingBlock: "8px",
    paddingInline: "14px",
    borderRadius: "lg",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    backgroundColor: "scrim",
    color: "fg.dim",
  }),
};

/**
 * Declared dimensions are a promise about the file; check it once per load.
 *
 * They exist so the page can reserve space before the image arrives. Nothing
 * downstream scales by them — but a set whose numbers are wrong is a set whose
 * mini-map and reserved space are wrong too, and that is invisible otherwise.
 */
function warnOnDimensionMismatch(
  element: HTMLImageElement,
  rendition: Rendition | undefined,
  imageLabel: string,
  configLabel: string,
): void {
  if (!rendition?.width || !rendition.height) return;
  if (element.naturalWidth === rendition.width && element.naturalHeight === rendition.height) {
    return;
  }
  console.warn(
    `${imageLabel} — ${configLabel}: review.json declares ` +
      `${rendition.width}x${rendition.height} but the image is ` +
      `${element.naturalWidth}x${element.naturalHeight}. The declared size only ` +
      `reserves space; fix it so the reserved box and mini-map match the picture.`,
  );
}

/**
 * How still the selection must be before the charts redraw.
 *
 * Long enough that toggling two configs back and forth draws none of them, short
 * enough that stopping feels immediate.
 */
const CHART_SETTLE_MS = 200;

interface Props {
  image: ReviewImage;
  configs: readonly ReviewConfig[];
  activeIndex: number;
  onActivate: (index: number) => void;
  zoom: ZoomMode;
  /** Whether the charts row is drawn at all — the `m` toggle, set-wide. */
  showMetrics: boolean;
  /** Whether this is the frame the viewer is looking at — see `currentFrame`. */
  isCurrent: boolean;
  /** Whether a review note has been written about this frame. */
  hasNote: boolean;
  /** Which pointer mode is on, set-wide — see `keys.ts`. */
  pointerMode: PointerMode;
  /** This frame's patches, in image coordinates. */
  patches: readonly Patch[];
  /** A rectangle drawn on *this* frame and waiting for its label. */
  pendingPatch: NormalRect | undefined;
  onDrawPatch: (rect: NormalRect) => void;
  onRemovePatch: (patchId: string) => void;
  /**
   * Watches the picture pane, which is what decides the answer above.
   *
   * The pane rather than the section: a section is a whole screen, so two are
   * nearly always in view, and which *picture* is on screen is the question.
   */
  paneObserver: IntersectionObserver | undefined;
}

/**
 * Overflow state — coarse, and deliberately *not* live scroll position.
 *
 * Scroll position drives the mini-map, which updates on every scroll event. Doing
 * that through a signal means every scroll frame re-renders, and a re-render that
 * changes layout feeds the next measurement: that cycle wedged the renderer so
 * hard Chrome could not inject a script into the page. So the mini-map is written
 * imperatively in the scroll handler, and only this — whether the viewport
 * scrolls at all — is reactive, changing just a few times per session.
 */
interface Overflow {
  readonly x: boolean;
  readonly y: boolean;
}

export function ImageSection(props: Props) {
  const [viewport, setViewport] = createSignal<HTMLDivElement>();
  // Handed back when this frame scrolls out of the mount window: the observer
  // holds its targets strongly, and sections churn as you scroll.
  let pane: HTMLDivElement | undefined;
  onCleanup(() => {
    if (pane) props.paneObserver?.unobserve(pane);
  });
  const [overflow, setOverflow] = createSignal<Overflow>(
    { x: false, y: false },
    { equals: (a, b) => a.x === b.x && a.y === b.y },
  );
  let mapWindow: HTMLDivElement | undefined;

  /*
    Where the picture is painted, for the pointer-mode overlay.

    Held as a signal because the overlay is positioned from it, and re-measured
    only by `measureOverflow` below — which is to say on load, resize, and the
    three things that change the stage's shape. **Not on scroll**: the overlay is
    inside the stage, so it is scrolled along with the picture and the box it
    sits at never changes. That is the whole reason it is anchored there rather
    than to the pane.
  */
  const [painted, setPainted] = createSignal<Box | undefined>(undefined, {
    equals: (a, b) =>
      a === b ||
      (a !== undefined &&
        b !== undefined &&
        a.x === b.x &&
        a.y === b.y &&
        a.width === b.width &&
        a.height === b.height),
  });
  let stage: HTMLDivElement | undefined;
  // The rendition elements, so a colour can be read back off the active one and
  // its painted box measured. Keyed by config id, and dropped with the section.
  const renditionElements = new Map<string, HTMLImageElement>();

  const measurePainted = () => {
    const id = activeId();
    const element = id === undefined ? undefined : renditionElements.get(id);
    if (!stage || !element || element.naturalWidth === 0) return setPainted(undefined);
    const stageBox = stage.getBoundingClientRect();
    const box = element.getBoundingClientRect();
    setPainted(
      paintedBox(
        {
          x: box.left - stageBox.left,
          y: box.top - stageBox.top,
          width: box.width,
          height: box.height,
        },
        element.naturalWidth,
        element.naturalHeight,
        props.zoom,
      ),
    );
  };

  /*
    The colour under the cursor, converted to the *pane's* coordinates here.

    Converted at sample time rather than at draw time so the readout component
    holds no measurement of its own: one `getBoundingClientRect` per pointer move
    either way, and this keeps the geometry out of a `.tsx` the test runner does
    not collect.
  */
  const [sample, setSample] = createSignal<
    { at: Point; pane: { width: number; height: number }; rgb: Rgb } | undefined
  >(undefined);
  // One state rather than a `copied` and a `failed` boolean, which between them
  // can encode "both at once".
  const [copyState, setCopyState] = createSignal<"idle" | "copied" | "failed">("idle");
  let clearCopied: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(clearCopied));

  // Leaving colour mode drops the reading with it: keeping it would flash the
  // previous pixel's chip, at the previous cursor position, on the next `i`.
  createEffect(
    on(
      () => props.pointerMode,
      () => setSample(undefined),
      { defer: true },
    ),
  );

  const takeSample = (reading: ColorSample | undefined) => {
    // A new reading is a new thing to copy, so a previous outcome must not
    // linger beside it and claim this colour was the one copied.
    if (copyState() !== "idle") {
      clearTimeout(clearCopied);
      setCopyState("idle");
    }
    if (!reading || !pane) return setSample(undefined);
    const box = pane.getBoundingClientRect();
    setSample({
      at: { x: reading.clientX - box.left, y: reading.clientY - box.top },
      pane: { width: box.width, height: box.height },
      rgb: reading.rgb,
    });
  };

  /**
   * A click in colour mode copies the HEX — the one value you retype elsewhere.
   *
   * **`try`/`catch` around the whole thing, not a `.catch` on the promise.**
   * `navigator.clipboard` is `undefined` outside a secure context — serving this
   * over a LAN address rather than `localhost` is enough — so reading
   * `.writeText` off it throws *synchronously*, before there is a promise to
   * reject, and a `.catch` never sees it.
   *
   * The outcome is shown in the readout rather than only logged: a silent
   * failure here is indistinguishable from a successful copy until you paste
   * something else, and a `console.warn` is invisible to whoever clicked.
   */
  const copyColor = async () => {
    const reading = sample();
    if (!reading) return;
    clearTimeout(clearCopied);
    let outcome: "copied" | "failed";
    try {
      await navigator.clipboard.writeText(toHex(reading.rgb));
      outcome = "copied";
    } catch (cause) {
      console.warn("Could not copy the colour", cause);
      outcome = "failed";
    }
    // **The outcome belongs to the reading that was clicked.** The write is
    // asynchronous, so the pointer can move while it is in flight — and the
    // guard in `takeSample` cannot help, because at that moment the state is
    // still `idle` and there is nothing for it to reset. Landing "copied"
    // beside a HEX that was never copied is the exact claim this must not make,
    // so a reading that has been replaced simply drops its result.
    if (sample() !== reading) return;
    setCopyState(outcome);
    clearCopied = setTimeout(() => setCopyState("idle"), 1200);
  };

  /** Position the mini-map's window box. Direct DOM write, no signals. */
  const paintMap = () => {
    const element = viewport();
    if (!element || !mapWindow) return;
    const pct = (part: number, whole: number) =>
      `${Math.min(100, (part / Math.max(whole, 1)) * 100)}%`;
    mapWindow.style.left = pct(element.scrollLeft, element.scrollWidth);
    mapWindow.style.top = pct(element.scrollTop, element.scrollHeight);
    mapWindow.style.width = pct(element.clientWidth, element.scrollWidth);
    mapWindow.style.height = pct(element.clientHeight, element.scrollHeight);
  };

  // Painted straight from the scroll handler. The browser already coalesces
  // scroll events to about one per frame, and this only reads scroll offsets and
  // writes four inline styles on the thumbnail-sized box — cheap enough not to need
  // throttling, and unlike `requestAnimationFrame` it still runs in a hidden or
  // backgrounded tab.
  /** Recheck whether the viewport scrolls. Cheap, and rare. */
  const measureOverflow = () => {
    const element = viewport();
    if (!element) return;
    setOverflow({
      x: element.scrollWidth - element.clientWidth > 1,
      y: element.scrollHeight - element.clientHeight > 1,
    });
    paintMap();
    measurePainted();
  };

  onMount(() => {
    measureOverflow();
    const onResize = () => measureOverflow();
    window.addEventListener("resize", onResize);
    onCleanup(() => window.removeEventListener("resize", onResize));
  });

  // Switching fit/fullsize resizes the stage, so re-check overflow. Measured
  // **synchronously** — Solid runs effects after the DOM is updated, so layout is
  // already current — with a follow-up frame for anything that settles late.
  // Correctness must not depend on `requestAnimationFrame`: a hidden or
  // backgrounded tab never fires it, which left the pan controls permanently
  // absent there.
  createEffect(
    on(
      // The active index matters as much as the zoom mode: `reservation()` is taken
      // from the *active* rendition, so switching to one that declares different
      // dimensions changes the stage's scroll size. Watching only the zoom left the
      // pan controls and mini-map stale until the next load, resize or toggle.
      // Hiding the charts is the same kind of change from the other side: the band
      // hands its height to the picture, so a `fullsize` pane that was scrolling
      // may stop, and one that fit may start.
      // The pointer mode joins them for a fourth reason: the overlay is mounted
      // only while a mode is on, so entering one must find a measured box rather
      // than waiting for the next resize.
      //
      // And the active rendition's **identity**, not just which config is
      // selected: a live refresh can replace or remove the picture under an
      // unchanged config, and a removal fires no `load` event to measure from.
      // Without it `painted` kept the vanished image's box, so the overlay
      // stayed mounted over a frame that had no rendition at all.
      () =>
        [
          props.zoom,
          props.activeIndex,
          props.showMetrics,
          props.pointerMode,
          activeRendition()?.src,
        ] as const,
      () => {
        measureOverflow();
        requestAnimationFrame(measureOverflow);
      },
    ),
  );

  const canPan = () => props.zoom === "fullsize" && (overflow().x || overflow().y);

  /** Step by most of a screen, keeping a sliver of overlap for continuity. */
  const pan = (dx: number, dy: number) => {
    const element = viewport();
    if (!element) return;
    element.scrollBy({
      left: dx * element.clientWidth * 0.8,
      top: dy * element.clientHeight * 0.8,
      // Smooth scrolling is driven by the same frame loop as `requestAnimationFrame`
      // and simply does not run in a hidden tab — the scroll would be silently
      // dropped. Fall back to an instant jump there so the action always happens.
      behavior: document.visibilityState === "visible" ? "smooth" : "auto",
    });
  };

  /*
    The charts lag the picture, on purpose.

    Swapping the picture is a class change on stacked `<img>`s and costs
    ~2ms; drawing three charts costs an order of magnitude more, and putting
    both in one synchronous handler makes the picture wait for numbers nobody
    has looked at yet. So the panel follows a **separate** config id that
    advances only once two things are true: the picture for it has loaded, and
    the selection has been still for `CHART_SETTLE_MS`. Toggling between two
    configs to compare them therefore draws no charts at all until you stop.

    The panel is fed the *charted* id throughout — rendition, label and chart
    ids — so it always describes the config it names, never the one on screen.
  */
  const [loaded, setLoaded] = createSignal<ReadonlySet<string>>(new Set<string>());
  const [charted, setCharted] = createSignal<string | undefined>(undefined);
  /**
   * How many loads have been *observed*, which is not how many configs are loaded.
   *
   * A counter rather than `loaded().size`, because the set answers "which configs
   * can be charted" and deliberately stops changing once a config is in it. On a
   * live refresh the rendition's URL changes and the same config loads again —
   * `markLoaded` then returns the identical set, and if the replacement is the
   * same size `painted` compares equal too, so nothing downstream could tell a
   * new picture had arrived. A colour reading taken while it was decoding stayed
   * blank until the pointer moved, in exactly the re-render-and-watch workflow
   * this app exists for.
   */
  const [loadTicks, setLoadTicks] = createSignal(0);
  const markLoaded = (configId: string) => {
    setLoadTicks((count) => count + 1);
    setLoaded((previous) => (previous.has(configId) ? previous : new Set(previous).add(configId)));
  };

  const activeId = () => props.configs[props.activeIndex]?.id;
  const activeRendition = () => {
    const id = activeId();
    return id === undefined ? undefined : props.image.renditions[id];
  };

  /**
   * Reserve the declared box in `fullsize`, before the image has loaded.
   *
   * A **floor**, not a size: the image still renders at its natural dimensions, so
   * a stale declared value cannot scale the picture (which is why `fullsize` sets
   * `width`/`height` to `natural` in the first place). Without this the stage is 0x0 until
   * the first decode and the whole section jumps when it lands. Not applied in
   * `fit`, where a 2400px floor would force horizontal overflow on a column that
   * is meant to shrink the image to fit.
   */
  const reservation = () => {
    const rendition = activeRendition();
    if (props.zoom !== "fullsize" || !rendition?.width || !rendition.height) {
      return undefined;
    }
    return {
      "min-width": `${rendition.width}px`,
      "min-height": `${rendition.height}px`,
    };
  };
  /**
   * Whether the picture for the selected config is up.
   *
   * **The active config's loadedness, not the whole set's.** Depending on
   * `loaded` itself re-runs the effect below whenever *any* rendition finishes,
   * which restarts the settle timer — on a set whose renditions arrive closer
   * together than `CHART_SETTLE_MS`, the charts would be starved until loading
   * happened to pause.
   */
  const activeLoaded = () => {
    const id = activeId();
    if (id === undefined) return false;
    // A config with no rendition has nothing to wait for, so the panel can still
    // clear for it.
    return !props.image.renditions[id] || loaded().has(id);
  };

  createEffect(
    on([() => props.activeIndex, activeLoaded], () => {
      const id = activeId();
      if (id === undefined || !activeLoaded()) return;
      const timer = setTimeout(() => setCharted(id), CHART_SETTLE_MS);
      onCleanup(() => clearTimeout(timer));
    }),
  );

  const chartedRendition = () => {
    const id = charted();
    return id === undefined ? undefined : props.image.renditions[id];
  };

  const hasActive = () => {
    const id = activeId();
    return id !== undefined && props.image.renditions[id] !== undefined;
  };

  return (
    // A `div`, not a `section`: the one-screen shell `App` wraps around this is
    // already the `<section>` for this frame, and nesting a second one inside it
    // announces two landmarks for one frame.
    <div class={css(styles.section)}>
      <div class={css(styles.head)}>
        <span class={css(styles.label, props.isCurrent && styles.labelCurrent)}>
          {props.image.label}
        </span>
        <Show when={props.image.note}>
          {(note) => <span class={css(styles.note)}>{note()}</span>}
        </Show>
        {/* Which frames you have already written about, visible while scrolling
            — otherwise the only way to tell is to open every note. */}
        <Show when={props.hasNote}>
          <span class={css(styles.noted)} title="This frame has a review note (a to edit)">
            ● noted
          </span>
        </Show>
        {/* Patches are drawn only in patch mode, so without this there is no way
            to tell a frame you have already marked up from one you have not. */}
        <Show when={props.patches.length > 0}>
          <span class={css(styles.patched)} title="Patches marked on this frame (p to show them)">
            ▢ {props.patches.length} {props.patches.length === 1 ? "patch" : "patches"}
          </span>
        </Show>
      </div>

      <div class={css(styles.strip)}>
        <For each={props.configs}>
          {(config, index) => {
            const rendition = () => props.image.renditions[config.id];
            return (
              <button
                type="button"
                class={css(styles.preview, index() === props.activeIndex && styles.previewActive)}
                aria-pressed={index() === props.activeIndex}
                title={`${config.label}${rendition() ? "" : " — no rendition for this image"}`}
                onClick={() => props.onActivate(index())}
              >
                <Show
                  when={rendition()}
                  fallback={
                    <span class={css(styles.previewMissing)}>
                      {config.label}
                      <br />
                      missing
                    </span>
                  }
                >
                  {(present) => (
                    <img
                      class={css(styles.previewImage)}
                      src={present().preview}
                      alt={`${props.image.label} — ${config.label}`}
                      loading="lazy"
                    />
                  )}
                </Show>
              </button>
            );
          }}
        </For>

        <Show when={canPan()}>
          <div
            class={css(styles.mapOuter)}
            title="Where the viewport sits inside the full-size image"
          >
            <div
              class={css(styles.mapWindow)}
              ref={(element) => {
                // Paint on attach: the map mounts *because* overflow appeared, so
                // the measurement that revealed it ran before this element existed.
                mapWindow = element;
                paintMap();
              }}
            />
          </div>
        </Show>
      </div>

      <div
        class={css(styles.frame)}
        data-pane
        ref={(element) => {
          pane = element;
          props.paneObserver?.observe(element);
        }}
      >
        <div class={css(styles.viewport)} ref={setViewport} onScroll={paintMap}>
          <div
            class={css(styles.stage, props.zoom === "fit" ? styles.stageFit : styles.stageFullsize)}
            style={reservation()}
            ref={(element) => (stage = element)}
          >
            <For each={props.configs}>
              {(config) => (
                <Show when={props.image.renditions[config.id]}>
                  {(rendition) => (
                    <img
                      class={css(
                        styles.rendition,
                        props.zoom === "fit" ? styles.fit : styles.fullsize,
                        config.id !== activeId() && styles.hidden,
                      )}
                      src={rendition().src}
                      width={rendition().width}
                      height={rendition().height}
                      alt={`${props.image.label} — ${config.label}`}
                      ref={(element) => {
                        // Held so a colour can be read back off the active one
                        // and its painted box measured. `For` disposes this scope
                        // when the config leaves the set, which is when it goes.
                        renditionElements.set(config.id, element);
                        onCleanup(() => renditionElements.delete(config.id));
                        // **A hydrated rendition fires no `load` event for us.**
                        // It arrived in the server-rendered HTML and finished
                        // before Solid attached the listener, so it must be
                        // recognised here or its charts never appear.
                        //
                        // `currentSrc` is what makes that check honest. On a
                        // client-created element Solid assigns `src` in a later
                        // effect, so at this point there is no source and
                        // `complete` is vacuously `true` — testing it alone
                        // marks every config loaded at mount and quietly retires
                        // the whole wait-for-the-picture rule.
                        if (element.complete && element.currentSrc !== "") {
                          markLoaded(config.id);
                        }
                      }}
                      onLoad={(event) => {
                        warnOnDimensionMismatch(
                          event.currentTarget,
                          rendition(),
                          props.image.label,
                          config.label,
                        );
                        markLoaded(config.id);
                        measureOverflow();
                      }}
                    />
                  )}
                </Show>
              )}
            </For>

            {/* Inside the stage, so it letterboxes with the picture in `fit` and
                scrolls with it in `fullsize` — see `StageOverlay`. Mounted only
                while a mode is on, and only once the picture has been measured:
                an overlay placed against an unknown box would sit on the wrong
                pixels, which is worse than not being there. */}
            <Show when={props.pointerMode !== "off" && painted()}>
              {(box) => (
                <StageOverlay
                  mode={props.pointerMode === "patch" ? "patch" : "color"}
                  painted={box()}
                  image={renditionElements.get(activeId() ?? "")}
                  patches={props.patches}
                  pendingPatch={props.pendingPatch}
                  onDraw={props.onDrawPatch}
                  onRemove={props.onRemovePatch}
                  loadedTick={loadTicks()}
                  onSample={takeSample}
                  onPick={() => void copyColor()}
                />
              )}
            </Show>
          </div>
        </div>

        {/* In the pane rather than in the overlay: the overlay is scrolled
            content and in `fullsize` is far bigger than the window, so a chip
            placed there could sit off screen. The pane is the picture's fixed
            frame, which is the box the readout must stay inside. */}
        <Show when={props.pointerMode === "color" && sample()}>
          {(reading) => (
            <ColorReadout
              rgb={reading().rgb}
              at={reading().at}
              pane={reading().pane}
              copyState={copyState()}
            />
          )}
        </Show>

        <Show when={!hasActive()}>
          <div class={css(styles.missing)}>
            No rendition of {props.image.label} for config{" "}
            {props.configs[props.activeIndex]?.label ?? "?"}.
          </div>
        </Show>

        <Show when={canPan()}>
          <Show when={overflow().y}>
            <button
              type="button"
              class={css(styles.pan, styles.panUp)}
              aria-label="Pan up"
              onClick={() => pan(0, -1)}
            >
              ↑
            </button>
            <button
              type="button"
              class={css(styles.pan, styles.panDown)}
              aria-label="Pan down"
              onClick={() => pan(0, 1)}
            >
              ↓
            </button>
          </Show>
          <Show when={overflow().x}>
            <button
              type="button"
              class={css(styles.pan, styles.panLeft)}
              aria-label="Pan left"
              onClick={() => pan(-1, 0)}
            >
              ←
            </button>
            <button
              type="button"
              class={css(styles.pan, styles.panRight)}
              aria-label="Pan right"
              onClick={() => pan(1, 0)}
            >
              →
            </button>
          </Show>
        </Show>
      </div>

      {/* Below the picture rather than beside it: the stage is the widest thing
          on the page and the charts must not narrow it. They swap with the
          config exactly as the picture does.

          Hidden by unmounting rather than by a style, because the cost `m` is
          put away for is the *drawing* — three SVGs per mounted frame — and only
          unmounting stops that. (`display: none` would free the band's height
          just as well, generating no box at all; `visibility: hidden` is the one
          that would hold `chartsBand` open. Neither stops the charts drawing.)
          What survives is `charted`, which lives in this component, so bringing
          them back draws the config you are on rather than restarting the wait. */}
      <Show when={props.showMetrics}>
        <div
          class={css(styles.band, chartedRendition()?.metrics !== undefined && styles.bandCharted)}
          data-charts-band
        >
          <MetricsPanel
            id={domId(props.image.id, charted() ?? "none")}
            rendition={chartedRendition()}
            configLabel={props.configs.find((config) => config.id === charted())?.label ?? "?"}
          />
        </div>
      </Show>
    </div>
  );
}
