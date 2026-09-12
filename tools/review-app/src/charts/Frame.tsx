import { For, Show } from "solid-js";
import { css } from "../../styled-system/css";
import type { Plot } from "./scale";

/**
 * The parts every chart here shares: gridlines, an axis, and the labels around
 * them. Nothing measured — see `scale.ts` for that.
 *
 * Colours go through `css()` rather than SVG `fill`/`stroke` attributes, because
 * Panda token-gates those two properties (`fill: Tokens["colors"]`). That keeps
 * chart chrome on the same light/dark semantic tokens as the rest of the app and
 * makes a mistyped colour a type error. The *data* colours are the exception and
 * stay attributes: a cast ramp encodes a CIELAB value, not a theme decision.
 */

const styles = {
  grid: css.raw({ stroke: "edge", strokeWidth: "1px", strokeDasharray: "2 4" }),
  gridStrong: css.raw({ stroke: "gridStrong", strokeWidth: "1px" }),
  axis: css.raw({ stroke: "edge", strokeWidth: "1px" }),
  tick: css.raw({ fill: "fg.dim", fontFamily: "mono", fontSize: "tick" }),
  tickStrong: css.raw({ fill: "fg", fontFamily: "mono", fontSize: "tick" }),
  label: css.raw({ fill: "fg.dim", fontSize: "key" }),
  caption: css.raw({ fill: "fg.dim", fontSize: "key" }),
};

interface GridProps {
  plot: Plot;
  /** Value and its pixel position, already scaled. */
  lines: readonly { value: number; at: number; strong?: boolean }[];
  orientation: "horizontal" | "vertical";
  format?: (value: number) => string;
}

/** Gridlines with their value labels, along one axis. */
export function Grid(props: GridProps) {
  return (
    <For each={props.lines}>
      {(line) => (
        <>
          <Show
            when={props.orientation === "horizontal"}
            fallback={
              <line
                x1={line.at}
                y1={props.plot.top}
                x2={line.at}
                y2={props.plot.bottom}
                class={css(line.strong ? styles.gridStrong : styles.grid)}
              />
            }
          >
            <line
              x1={props.plot.left}
              y1={line.at}
              x2={props.plot.right}
              y2={line.at}
              class={css(line.strong ? styles.gridStrong : styles.grid)}
            />
          </Show>
          <Show when={props.format}>
            {(format) => (
              <Show
                when={props.orientation === "horizontal"}
                fallback={
                  <text
                    x={line.at}
                    y={props.plot.bottom + 16}
                    text-anchor="middle"
                    class={css(line.strong ? styles.tickStrong : styles.tick)}
                  >
                    {format()(line.value)}
                  </text>
                }
              >
                <text
                  x={props.plot.left - 7}
                  y={line.at + 4}
                  text-anchor="end"
                  class={css(line.strong ? styles.tickStrong : styles.tick)}
                >
                  {format()(line.value)}
                </text>
              </Show>
            )}
          </Show>
        </>
      )}
    </For>
  );
}

/** The baseline under a plot, and the caption naming what its x axis measures. */
export function AxisLine(props: { plot: Plot; caption: string }) {
  return (
    <>
      <line
        x1={props.plot.left}
        y1={props.plot.bottom}
        x2={props.plot.right}
        y2={props.plot.bottom}
        class={css(styles.axis)}
      />
      <text
        x={(props.plot.left + props.plot.right) / 2}
        y={props.plot.bottom + 36}
        text-anchor="middle"
        class={css(styles.caption)}
      >
        {props.caption}
      </text>
    </>
  );
}

/** A rotated label down the left-hand side, naming what the y axis measures. */
export function AxisTitle(props: { plot: Plot; text: string }) {
  return (
    <text
      transform={`translate(14, ${(props.plot.top + props.plot.bottom) / 2}) rotate(-90)`}
      text-anchor="middle"
      class={css(styles.label)}
    >
      {props.text}
    </text>
  );
}

export const frameStyles = styles;
