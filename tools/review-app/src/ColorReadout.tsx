import { Match, Switch } from "solid-js";
import { css } from "../styled-system/css";
import { type Rgb, chroma, describeColor, lightness, toHex } from "./color";
import { type Point, placeReadout } from "./geometry";
import { token } from "../styled-system/tokens";

/**
 * The colour under the cursor, beside the cursor.
 *
 * Rendered in the **pane**, not in the overlay that measures it. The overlay is
 * part of the scrolled content and in `fullsize` is far larger than the window,
 * so a chip placed in it could sit off screen; the pane is the picture's fixed
 * frame, which is the box the chip has to stay inside. `placeReadout` flips it
 * around the cursor at the far edges rather than sliding it, so it never covers
 * the pixel it is describing.
 *
 * Its size is **stated**, not measured, because the flip arithmetic needs it
 * before layout — and measuring it would put a read of the element in the path
 * of the write that positions it, the feedback shape this app has been bitten by.
 * The two tokens are therefore a contract with this component's contents.
 *
 * **Both numbers are the ones the phrase above them is derived from** — L\* for
 * the lightness word, chroma for how far from neutral it is. HSL saturation was
 * the obvious thing to print and is the wrong thing: it divides by the room a
 * colour of that lightness has, so a near-white with a faint warm cast reports
 * "slightly yellow" beside a saturation of 39%, and the word and the number read
 * as a contradiction. That is the same trap the lightness word was moved to L\*
 * to avoid; see `color.ts`.
 */

const READOUT = {
  width: Number.parseFloat(token("sizes.readoutWidth")),
  height: Number.parseFloat(token("sizes.readoutHeight")),
};
const READOUT_GAP = 16;

const styles = {
  chip: css.raw({
    position: "absolute",
    zIndex: 4,
    width: "readoutWidth",
    height: "readoutHeight",
    boxSizing: "border-box",
    display: "flex",
    gap: "8px",
    alignItems: "center",
    paddingBlock: "8px",
    paddingInline: "10px",
    borderRadius: "lg",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    backgroundColor: "scrim",
    color: "fg",
    // It follows the pointer, so it must never be the thing under it.
    pointerEvents: "none",
  }),
  swatch: css.raw({
    width: "swatch",
    height: "swatch",
    flexShrink: 0,
    borderRadius: "sm",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
  }),
  text: css.raw({ display: "flex", flexDirection: "column", gap: "2px", minWidth: "zero" }),
  hex: css.raw({ fontFamily: "mono", fontSize: "code", fontWeight: "semibold" }),
  name: css.raw({
    fontSize: "meta",
    whiteSpace: "nowrap",
    overflow: "hidden",
    textOverflow: "ellipsis",
  }),
  numbers: css.raw({
    fontFamily: "mono",
    fontSize: "key",
    color: "fg.dim",
    fontVariantNumeric: "tabular-nums",
  }),
  copied: css.raw({ fontSize: "key", color: "accent" }),
  failed: css.raw({ fontSize: "key", color: "bad" }),
};

export function ColorReadout(props: {
  rgb: Rgb;
  /** Where the cursor is, in the pane's coordinates. */
  at: Point;
  pane: { width: number; height: number };
  copyState: "idle" | "copied" | "failed";
}) {
  const hex = () => toHex(props.rgb);
  const place = () => placeReadout(props.at, props.pane, READOUT, READOUT_GAP);
  return (
    <div
      class={css(styles.chip)}
      style={{ left: `${place().left}px`, top: `${place().top}px` }}
      aria-live="off"
    >
      <div class={css(styles.swatch)} style={{ "background-color": hex() }} />
      <div class={css(styles.text)}>
        <span class={css(styles.hex)}>{hex()}</span>
        <span class={css(styles.name)}>{describeColor(props.rgb)}</span>
        {/* Two lines, not one. At the chip's stated width a single line wraps
            mid-number in mono, and a wrap the layout did not plan for is what
            pushes the hint below the box. */}
        <span class={css(styles.numbers)}>
          {props.rgb.r} {props.rgb.g} {props.rgb.b}
        </span>
        <span class={css(styles.numbers)}>
          L* {lightness(props.rgb).toFixed(1)} · chroma {Math.round(chroma(props.rgb) * 100)}%
        </span>
        <Switch fallback={<span class={css(styles.numbers)}>click to copy</span>}>
          <Match when={props.copyState === "copied"}>
            <span class={css(styles.copied)}>copied</span>
          </Match>
          <Match when={props.copyState === "failed"}>
            <span class={css(styles.failed)}>copy failed</span>
          </Match>
        </Switch>
      </div>
    </div>
  );
}
