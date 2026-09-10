import * as stylex from "@stylexjs/stylex";
import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { cls } from "./cls";
import { ControlBar } from "./ControlBar";
import { ImageSection } from "./ImageSection";
import { actionForKey, isTextEntry } from "./keys";
import type { ReviewSetPayload } from "./server/getReviewSet";
import { type ZoomMode } from "./review";

const styles = stylex.create({
  header: { paddingBlock: 18, paddingInline: 16 },
  title: { marginBlock: 0, fontSize: 18 },
  description: { marginBlockStart: 4, color: "var(--fg-dim)", maxWidth: "78ch" },
  panel: { padding: 24, maxWidth: "80ch" },
  source: {
    marginBlockStart: 6,
    color: "var(--fg-dim)",
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    fontSize: 12,
    wordBreak: "break-all",
  },
});

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
      <header class={cls(styles.header)}>
        <Show when={review().title}>{(title) => <h1 class={cls(styles.title)}>{title()}</h1>}</Show>
        <Show when={review().description}>
          {(description) => <p class={cls(styles.description)}>{description()}</p>}
        </Show>
        {/* There is no longer a URL saying which set this is, so the page does. */}
        <p class={cls(styles.source)}>
          <Show when={props.set.source === "bundled-example"} fallback={props.set.path}>
            bundled example — set REVIEW_SET, or run `pnpm dev {"<path to review.json>"}`, to review
            your own
          </Show>
        </p>
      </header>
      <For each={review().images}>
        {(image) => (
          <ImageSection
            image={image}
            configs={review().configs}
            activeIndex={activeIndex()}
            onActivate={setActiveIndex}
            zoom={zoom()}
          />
        )}
      </For>
      <Show when={review().images.length === 0}>
        <p class={cls(styles.panel)}>This review set lists no images.</p>
      </Show>
    </main>
  );
}
