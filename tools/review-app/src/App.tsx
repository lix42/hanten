import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { css } from "../styled-system/css";
import { ControlBar } from "./ControlBar";
import { ImageSection } from "./ImageSection";
import { actionForKey, isTextEntry } from "./keys";
import type { ReviewSetPayload } from "./server/getReviewSet";
import { type ZoomMode } from "./review";

const styles = {
  header: css.raw({ paddingBlock: "18px", paddingInline: "16px" }),
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
};

export function App(props: { set: ReviewSetPayload }) {
  const review = createMemo(() => props.set.review);
  const [zoom, setZoom] = createSignal<ZoomMode>("fit");

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

  onMount(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isTextEntry(event.target)) return;
      const action = actionForKey(
        event.key,
        { alt: event.altKey, ctrl: event.ctrlKey, meta: event.metaKey },
        review().configs.length,
      );
      if (!action) return;
      event.preventDefault();
      if (action.kind === "config") setActiveIndex(action.index);
      else setZoom((current) => (current === "fit" ? "fullsize" : "fit"));
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
          return (
            <Show when={image()}>
              {(present) => (
                <ImageSection
                  image={present()}
                  configs={review().configs}
                  activeIndex={activeIndex()}
                  onActivate={setActiveIndex}
                  zoom={zoom()}
                />
              )}
            </Show>
          );
        }}
      </For>
      <Show when={review().images.length === 0}>
        <p class={css(styles.panel)}>This review set lists no images.</p>
      </Show>
    </main>
  );
}
