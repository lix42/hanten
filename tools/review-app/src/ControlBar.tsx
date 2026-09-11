import { For, Show } from "solid-js";
import { css } from "../styled-system/css";
import { keyForConfigIndex } from "./keys";
import type { ReviewConfig, ZoomMode } from "./review";

// Border longhands rather than the `border` shorthand: `active` overrides only
// the colour, and `css()` merges by property — a base that spelled the whole
// border as one shorthand would leave the override as a second, competing
// declaration.
const styles = {
  bar: css.raw({
    position: "sticky",
    top: "0",
    zIndex: 10,
    display: "flex",
    flexWrap: "wrap",
    gap: "16px",
    alignItems: "center",
    paddingBlock: "10px",
    paddingInline: "16px",
    backgroundColor: "panel",
    borderBottomWidth: "1px",
    borderBottomStyle: "solid",
    borderBottomColor: "edge",
  }),
  group: css.raw({ display: "flex", flexWrap: "wrap", gap: "6px", alignItems: "center" }),
  spacer: css.raw({ marginInlineStart: "auto" }),
  button: css.raw({
    display: "inline-flex",
    alignItems: "baseline",
    gap: "6px",
    paddingBlock: "5px",
    paddingInline: "12px",
    borderRadius: "md",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    backgroundColor: "button",
    color: "fg",
    // Stated rather than inherited: these are the page's own body font and size,
    // which is what a control should match, and `inherit` is not a token.
    fontFamily: "body",
    fontSize: "body",
    cursor: "pointer",
  }),
  active: css.raw({
    backgroundColor: "accent",
    borderColor: "accent",
    color: "accent.ink",
    fontWeight: "semibold",
  }),
  // `<kbd>` defaults to the UA's monospace font, which differs per platform and
  // per element; state it so the key sits on the label's baseline predictably.
  // Colour is inherited so one rule reads correctly on both the plain and the
  // filled (active) button.
  key: css.raw({
    fontFamily: "mono",
    fontSize: "key",
    opacity: 0.7,
    fontVariantNumeric: "tabular-nums",
  }),
  hint: css.raw({ fontFamily: "mono", color: "fg.dim", fontSize: "meta" }),
};

interface Props {
  configs: readonly ReviewConfig[];
  activeIndex: number;
  onActivate: (index: number) => void;
  zoom: ZoomMode;
  onZoom: (zoom: ZoomMode) => void;
}

export function ControlBar(props: Props) {
  return (
    <div class={css(styles.bar)}>
      <div class={css(styles.group)}>
        <For each={props.configs}>
          {(config, index) => {
            const shortcut = () => keyForConfigIndex(index());
            return (
              <button
                type="button"
                class={css(styles.button, index() === props.activeIndex && styles.active)}
                aria-pressed={index() === props.activeIndex}
                // The real ARIA spelling of what the `<kbd>` shows, so the shortcut
                // is announced as a shortcut rather than read as part of the name.
                aria-keyshortcuts={shortcut()}
                title={config.note ?? config.label}
                onClick={() => props.onActivate(index())}
              >
                <span>{config.label}</span>
                {/* Past the number row there is no key, and an empty <kbd> claims
                    a keystroke that does not exist — so render none. */}
                <Show when={shortcut()}>
                  {(key) => (
                    <kbd class={css(styles.key)} aria-hidden="true">
                      {key()}
                    </kbd>
                  )}
                </Show>
              </button>
            );
          }}
        </For>
      </div>

      <div class={css(styles.group, styles.spacer)}>
        <For each={["fullsize", "fit"] as const}>
          {(mode) => (
            <button
              type="button"
              class={css(styles.button, props.zoom === mode && styles.active)}
              aria-pressed={props.zoom === mode}
              aria-keyshortcuts="f"
              title={`Show images at ${mode === "fit" ? "fit" : "natural"} size (f toggles)`}
              onClick={() => props.onZoom(mode)}
            >
              {mode}
            </button>
          )}
        </For>
        <kbd class={css(styles.hint)} aria-hidden="true">
          f
        </kbd>
      </div>
    </div>
  );
}
