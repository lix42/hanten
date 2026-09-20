import {
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  on,
  onCleanup,
  onMount,
} from "solid-js";
import { css } from "../styled-system/css";
import { token } from "../styled-system/tokens";
import { ControlBar } from "./ControlBar";
import { ImageSection } from "./ImageSection";
import {
  actionForKey,
  isTextEntry,
  nextPointerMode,
  stepConfigIndex,
  type PointerMode,
} from "./keys";
import type { NormalRect } from "./geometry";
import { watchPointer } from "./pointer";
import { addPatch, patchCount, patchesFor, removePatch, type Patch, type Patches } from "./patches";
import type { ReviewSetPayload } from "./server/getReviewSet";
import { type ZoomMode } from "./review";
import { currentFrame, frameForKey, mountWindow, sameIds } from "./visible";
import { ClearDialog, NoteDialog, NotesDialog, PatchDialog } from "./NoteDialogs";
import { noteCount, notesScope, setNote, type NotedFrame, type Notes } from "./notes";

const styles = {
  // A snap stop of its own. Without one, the nearest snap point from the top of
  // the page is the first frame, so scrolling up to read the title or the source
  // path is pulled straight back down past them.
  header: css.raw({ paddingBlock: "18px", paddingInline: "16px", scrollSnapAlign: "start" }),
  title: css.raw({ marginBlock: "0", fontSize: "heading" }),
  description: css.raw({ marginBlockStart: "4px", color: "fg.dim", maxWidth: "proseMeasure" }),
  panel: css.raw({ padding: "24px", maxWidth: "panelMeasure" }),
  source: css.raw({
    marginBlockStart: "6px",
    color: "fg.dim",
    fontFamily: "mono",
    fontSize: "meta",
    wordBreak: "break-all",
  }),

  /*
    One frame, one screen — and **the shell is this tall whether or not its
    contents are mounted**. That invariant is what makes mounting safe to drive
    from an observer: mounting a frame changes nothing about the page's layout,
    so it cannot move a sibling across the viewport edge and trigger the next
    round of mounting. It also means the scrollbar is right from the first paint
    rather than growing as pictures arrive.

    It retires a limit too. The metrics panel's height used to depend on the
    active config, so switching config resized every section above the viewport
    and a browser without scroll anchoring moved the picture you were reading.
    Every section is now the same height regardless of what is in it.
  */
  frame: css.raw({ height: "frameHeight", scrollSnapAlign: "start", overflow: "hidden" }),
  placeholder: css.raw({
    height: "full",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    color: "fg.dim",
  }),
};

/**
 * Viewport y that a frame's top snaps to: the sticky control bar's lower edge.
 *
 * **Read from the same token CSS uses, not measured off the bar.** `frameHeight`
 * subtracts `barHeight` and `scroll-padding-top` is that token, so a measured
 * value is a second source of truth for one number: let the two diverge and the
 * browser's own snapping pulls the page a few pixels after `align` lands, which
 * makes `isAligned` permanently false and turns `j` into a re-align loop rather
 * than a step. Reading the token cannot diverge, and it retires the `?? 0`
 * fallback that would silently have aligned every frame under the bar.
 */
const SNAP_LINE = Number.parseFloat(token("sizes.barHeight"));

/**
 * Patch ids, from a counter rather than from the content.
 *
 * Two patches on one frame may legitimately carry the same label and very nearly
 * the same rectangle — "white 1" and "white 2" on one shirt is the expected use —
 * so anything derived from a patch's own fields could collide, and deleting one
 * would delete both. A counter cannot, and nothing outside the page ever reads it.
 */
let patchSerial = 0;
const nextPatchId = () => `patch-${(patchSerial += 1)}`;

export function App(props: { set: ReviewSetPayload }) {
  const review = createMemo(() => props.set.review);
  const [zoom, setZoom] = createSignal<ZoomMode>("fit");
  // Whether the charts row is drawn, for every frame at once rather than per
  // frame: it is a band of the one-screen layout, and a set where some frames
  // showed charts and others did not would move the picture as you stepped
  // through them — the thing this page is arranged to prevent.
  const [showMetrics, setShowMetrics] = createSignal(true);

  // The selection is held as a config **id**, not an index. A live edit to
  // `review.json` can insert, remove or reorder configs, and a retained index
  // then points at a different one — or at nothing, leaving the page with no
  // selection. Holding the id keeps "the same config stays selected" true, which
  // is the whole promise of reloading in place.
  const [activeId, setActiveId] = createSignal<string | undefined>(undefined);
  const activeIndex = createMemo(() => {
    const configs = review().configs;
    const id = activeId();
    const found = id === undefined ? -1 : configs.findIndex((config) => config.id === id);
    // The selected config was removed, or nothing is chosen yet: fall back to
    // the first, which is always present (`configs` is never empty).
    return found === -1 ? 0 : found;
  });
  // A stable list, so `For` only adds and removes sections when the set's images
  // actually change rather than on every refresh.
  const imageIds = createMemo(() => review().images.map((image) => image.id), undefined, {
    equals: (a, b) => a.length === b.length && a.every((id, i) => id === b[i]),
  });

  const setActiveIndex = (index: number) => {
    setActiveId(review().configs[index]?.id);
  };

  /*
    Which frames the viewport can see, and therefore which ones render a picture.

    A signal written from an observer callback, which looks like the scroll
    handler this app forbids — and is not. That rule exists because scroll ->
    signal -> re-render -> layout change -> measure is a cycle; here the shell's
    height is fixed above, so a render triggered by this callback changes no
    geometry and cannot feed the next callback. It also fires a handful of times
    per scroll rather than once a frame.

    `IntersectionObserver` is absent when this renders on the server, where the
    empty set makes `mountWindow` mount the head of the list — so the markup that
    ships is the frames you are about to look at, and the client hydrates the same
    thing before the first callback arrives.
  */
  const [visible, setVisible] = createSignal<ReadonlySet<string>>(new Set<string>(), {
    equals: sameIds,
  });
  const observer =
    typeof IntersectionObserver === "undefined"
      ? undefined
      : new IntersectionObserver((entries) => {
          setVisible((current) => {
            const next = new Set(current);
            for (const entry of entries) {
              const id = entry.target.getAttribute("data-image-id");
              if (id === null) continue;
              if (entry.isIntersecting) next.add(id);
              else next.delete(id);
            }
            return next;
          });
        });
  onCleanup(() => observer?.disconnect());

  const mounted = createMemo(() => mountWindow(imageIds(), visible()));

  /*
    The frame the viewer is looking at, and how `j`/`k` move it.

    Two different readings of the same rule, on purpose. The **mark** is driven
    by an observer on each picture pane, because it has to follow a free scroll.
    The **keys** re-read the rule straight off the DOM at keypress time instead:
    an observer callback is asynchronous, so two quick presses of `j` would both
    act on the same stale answer and the second would scroll backwards. A
    keypress is not a scroll handler, so measuring in it costs nothing.

    That measurement is only trustworthy because `align` scrolls **instantly** —
    a page mid-animation reports both the frame being left and the one being
    entered, and the rule would answer with the wrong one. `currentFrame` is the
    one rule both readings go through.
  */
  const sections = new Map<string, Element>();
  const [current, setCurrent] = createSignal<string | undefined>(undefined);

  /** Frames whose picture is on screen, measured now. */
  const picturesOnScreen = () => {
    const found = new Set<string>();
    for (const [id, section] of sections) {
      const pane = section.querySelector("[data-pane]");
      if (!pane) continue; // an unmounted frame has no picture to show
      const box = pane.getBoundingClientRect();
      if (box.bottom > SNAP_LINE && box.top < window.innerHeight) found.add(id);
    }
    return found;
  };

  const readCurrent = () =>
    setCurrent((previous) => currentFrame(imageIds(), picturesOnScreen(), previous));

  // Safe for the same reason the mounting observer is: what this re-renders is
  // an underline, which takes no space, so it cannot move a pane across the
  // viewport edge and call this again.
  const paneObserver =
    typeof IntersectionObserver === "undefined" ? undefined : new IntersectionObserver(readCurrent);
  onCleanup(() => paneObserver?.disconnect());

  const isAligned = (id: string | undefined) => {
    const section = id === undefined ? undefined : sections.get(id);
    if (!section) return false;
    // A tolerance, not equality: a free scroll and fractional device pixels both
    // land a fraction off, and being one pixel short must not cost a press.
    return Math.abs(section.getBoundingClientRect().top - SNAP_LINE) <= 2;
  };

  const align = (id: string) => {
    const section = sections.get(id);
    if (!section) return;
    window.scrollTo({
      top: section.getBoundingClientRect().top + window.scrollY - SNAP_LINE,
      // **Instant, and that is what makes the keys correct.** A smooth scroll is
      // still running when the next key arrives, and the rule below reads the
      // DOM: mid-animation both the frame being left and the one being entered
      // have a picture on screen, so `currentFrame` answers with the *lower* —
      // the one you are leaving — and a second `k` scrolled back down to where
      // it started. Landing immediately means every press measures a settled
      // page. It also sidesteps smooth scrolling being dropped in a hidden tab.
      behavior: "auto",
    });
    setCurrent(id);
  };

  /*
    Review notes, held in the page and nowhere else.

    Cleared automatically when `notesScope` changes — a different set, or one
    whose frame list changed. Explicitly **not** on every refresh: re-running
    `nc` over the same frames and watching them update in place is the workflow
    this app exists for, and wiping the notes each time would make them useless.
    A note that outlives its picture is the lesser problem, and `c` is there for
    the rest.
  */
  /*
    The pointer modes, `p` and `i`, as **one selection rather than two flags**.

    Each takes over the pointer on the picture — one draws rectangles, the other
    reads pixels — so a pair that could both be on is a state with no sensible
    behaviour. Holding one value makes it unrepresentable instead of having to be
    checked, and each key toggles its own mode through `nextPointerMode`.

    Set-wide, like the charts toggle: a frame is a whole screen, so the mode you
    are in is the mode you are in, and per-frame modes would mean the picture
    behaved differently depending on how far you had scrolled.
  */
  const [pointerMode, setPointerMode] = createSignal<PointerMode>("off");

  /*
    Patches: labelled rectangles, keyed by image id and shared by every config of
    that frame — see `patches.ts`. Held here beside the notes because they are
    cleared by the same rule, copied in the same block, and drawn on frames that
    mount and unmount as you scroll.
  */
  const [patches, setPatches] = createSignal<Patches>({});
  /**
   * The rectangle just drawn, waiting for its label.
   *
   * Held with the frame it belongs to, for the same reason `noteTarget` is: the
   * dialog is modal and the page can scroll behind it, so "which frame" must be
   * decided when the drag ends rather than read again when the label is typed.
   */
  const [pendingPatch, setPendingPatch] = createSignal<
    { imageId: string; rect: NormalRect } | undefined
  >(undefined);

  const [notes, setNotes] = createSignal<Notes>({});
  const [noteOpen, setNoteOpen] = createSignal(false);
  /**
   * The frame `a` was pressed on, held for the dialog's lifetime.
   *
   * Not `current()`: that is written asynchronously by the pane observer, so a
   * scroll behind the open dialog would move it and every further keystroke
   * would rewrite a *different* frame's note — invisibly, since the textarea is
   * uncontrolled and goes on showing the first frame's text.
   */
  const [noteTarget, setNoteTarget] = createSignal<string | undefined>(undefined);
  const [notesOpen, setNotesOpen] = createSignal(false);
  const [clearOpen, setClearOpen] = createSignal(false);
  const dialogOpen = () => noteOpen() || notesOpen() || clearOpen() || pendingPatch() !== undefined;

  const frames = createMemo<NotedFrame[]>(() =>
    review().images.map((image) => ({ id: image.id, label: image.label })),
  );
  const editNote = (id: string, text: string) =>
    setNotes((previous) => setNote(previous, id, text));

  createEffect(
    on(
      () => notesScope(props.set.path, imageIds()),
      () => {
        setNotes({});
        // Patches go for the stronger version of the same reason: a note about a
        // frame that is gone is merely stale, while a rectangle is a claim about
        // *where* something is — on another frame it points at the wrong thing.
        setPatches({});
        setPendingPatch(undefined);
        // Back to the top with them. The scroll position is held against the
        // *frames*, and this is the one change that invalidates it: a different
        // set, or a set whose frame list moved, leaves you parked at an offset
        // that now lands on some other frame — or past the end of a shorter set,
        // where the browser clamps you to the bottom with no way to tell what
        // you are looking at. Instant, like `align`, so the frame mark below is
        // read off a settled page.
        window.scrollTo({ top: 0, behavior: "auto" });
      },
      { defer: true },
    ),
  );

  // Remembered from the first move onward, because the colour readout has to be
  // able to answer at moments when the pointer has not moved — see `pointer.ts`.
  onMount(() => onCleanup(watchPointer()));

  onMount(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isTextEntry(event.target)) return;
      // A dialog owns the keyboard while it is up; Esc is the platform's.
      if (dialogOpen()) return;
      const action = actionForKey(
        event.key,
        { alt: event.altKey, ctrl: event.ctrlKey, meta: event.metaKey },
        review().configs.length,
      );
      if (!action) return;
      event.preventDefault();
      if (action.kind === "config") {
        setActiveIndex(action.index);
      } else if (action.kind === "zoom") {
        setZoom((mode) => (mode === "fit" ? "fullsize" : "fit"));
      } else if (action.kind === "configStep") {
        setActiveIndex(stepConfigIndex(activeIndex(), action.delta, review().configs.length));
      } else if (action.kind === "note") {
        // Only ever about a frame that is on screen, so `a` cannot quietly
        // annotate one scrolled far away.
        const here = currentFrame(imageIds(), picturesOnScreen(), current());
        if (here === undefined) return;
        setCurrent(here);
        setNoteTarget(here);
        setNoteOpen(true);
      } else if (action.kind === "notes") {
        setNotesOpen(true);
      } else if (action.kind === "clearNotes") {
        // Patches count too: `c` is "throw away what I wrote on this review",
        // and a rectangle is as much of that as a sentence is.
        if (noteCount(notes()) + patchCount(patches()) > 0) setClearOpen(true);
      } else if (action.kind === "pointerMode") {
        setPointerMode((mode) => nextPointerMode(mode, action.mode));
      } else if (action.kind === "metrics") {
        setShowMetrics((shown) => !shown);
      } else {
        const here = currentFrame(imageIds(), picturesOnScreen(), current());
        const target = frameForKey(imageIds(), here, action.delta, isAligned(here));
        if (target !== undefined) align(target);
      }
    };
    document.addEventListener("keydown", onKeyDown);
    onCleanup(() => document.removeEventListener("keydown", onKeyDown));
  });

  return (
    <main>
      <ControlBar
        configs={review().configs}
        activeIndex={activeIndex()}
        onActivate={setActiveIndex}
        zoom={zoom()}
        onZoom={setZoom}
        pointerMode={pointerMode()}
        onPointerMode={(mode) => setPointerMode((current) => nextPointerMode(current, mode))}
      />
      <header class={css(styles.header)}>
        <Show when={review().title}>{(title) => <h1 class={css(styles.title)}>{title()}</h1>}</Show>
        <Show when={review().description}>
          {(description) => <p class={css(styles.description)}>{description()}</p>}
        </Show>
        {/* There is no longer a URL saying which set this is, so the page does. */}
        <p class={css(styles.source)}>
          <Show when={props.set.source === "bundled-example"} fallback={props.set.path}>
            bundled example — set REVIEW_SET, or run `pnpm dev {"<path to review.json>"}`, to review
            your own
          </Show>
        </p>
      </header>
      {/*
        Keyed by image id, not by the image objects themselves. A refresh
        reparses the set into fresh objects, and `For` reconciles by reference —
        so every `ImageSection` would be disposed and rebuilt, resetting the pan
        position inside each one. Since re-running `nc` while panned into a
        frame is exactly the workflow this exists for, the sections have to
        survive their data changing. Reading the image through a memo keeps the
        component alive and lets its renditions update underneath it.
      */}
      <For each={imageIds()}>
        {(id) => {
          const image = createMemo(() => review().images.find((i) => i.id === id));
          let element: Element | undefined;
          // `For` disposes this scope when the image leaves the set; the
          // observer holds a strong reference to its targets, so hand it back.
          onCleanup(() => {
            sections.delete(id);
            if (element) observer?.unobserve(element);
          });
          return (
            <Show when={image()}>
              {(present) => (
                <section
                  class={css(styles.frame)}
                  data-image-id={id}
                  ref={(node) => {
                    element = node;
                    sections.set(id, node);
                    observer?.observe(node);
                  }}
                >
                  <Show
                    when={mounted().has(id)}
                    fallback={<div class={css(styles.placeholder)}>{present().label}</div>}
                  >
                    <ImageSection
                      image={present()}
                      configs={review().configs}
                      activeIndex={activeIndex()}
                      onActivate={setActiveIndex}
                      zoom={zoom()}
                      showMetrics={showMetrics()}
                      isCurrent={current() === id}
                      hasNote={(notes()[id] ?? "").trim() !== ""}
                      pointerMode={pointerMode()}
                      patches={patchesFor(patches(), id)}
                      pendingPatch={
                        pendingPatch()?.imageId === id ? pendingPatch()?.rect : undefined
                      }
                      onDrawPatch={(rect) => setPendingPatch({ imageId: id, rect })}
                      onRemovePatch={(patchId) =>
                        setPatches((previous) => removePatch(previous, id, patchId))
                      }
                      paneObserver={paneObserver}
                    />
                  </Show>
                </section>
              )}
            </Show>
          );
        }}
      </For>
      <Show when={review().images.length === 0}>
        <p class={css(styles.panel)}>This review set lists no images.</p>
      </Show>

      <NoteDialog
        open={noteOpen()}
        onClose={() => setNoteOpen(false)}
        frame={frames().find((frame) => frame.id === noteTarget())}
        note={notes()[noteTarget() ?? ""] ?? ""}
        onInput={(text) => {
          const id = noteTarget();
          if (id !== undefined) editNote(id, text);
        }}
      />
      <NotesDialog
        open={notesOpen()}
        onClose={() => setNotesOpen(false)}
        frames={frames()}
        notes={notes()}
        patches={patches()}
        title={review().title}
        onInput={editNote}
      />
      <ClearDialog
        open={clearOpen()}
        onClose={() => setClearOpen(false)}
        notes={noteCount(notes())}
        patches={patchCount(patches())}
        onConfirm={() => {
          setNotes({});
          setPatches({});
        }}
      />
      {/* Cancelling is what removes a just-drawn patch, which is why the dialog
          has no other way out: the rectangle exists only once it is labelled. */}
      <PatchDialog
        open={pendingPatch() !== undefined}
        frame={frames().find((frame) => frame.id === pendingPatch()?.imageId)}
        onCancel={() => setPendingPatch(undefined)}
        onAdd={(label) => {
          const drawn = pendingPatch();
          setPendingPatch(undefined);
          if (!drawn) return;
          const patch: Patch = { id: nextPatchId(), label, ...drawn.rect };
          setPatches((previous) => addPatch(previous, drawn.imageId, patch));
        }}
      />
    </main>
  );
}
