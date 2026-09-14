import { For, Show, createMemo } from "solid-js";
import { css } from "../../styled-system/css";
import { AxisLine, AxisTitle, Grid } from "./Frame";
import type { CastBand } from "./metrics";
import { type CastAxis, rampAt, rampGradientStops, rampSpan } from "./ramp";
import {
  extent,
  linearScale,
  niceStep,
  paddedBounds,
  plotArea,
  polyline,
  populationRadius,
  slotCentres,
  ticks,
} from "./scale";

/**
 * Colour cast across the tone bands: `a*` and `b*`, each line coloured by its
 * own value.
 *
 * `a*` runs green below zero to red above, `b*` blue below to yellow above, so
 * the line teaches its own axis and nothing has to be memorised. The stroke is a
 * gradient in user space, which makes colour a function of y position — and y
 * position *is* the value, so the two cannot disagree.
 *
 * Two rules the ramp carries, both in `ramp.ts`: the value-to-colour map is the
 * same on every frame up to `RAMP_REFERENCE`, so a mild cast and a severe one do
 * not paint alike; and a mark and the line beneath it share one mapping, or the
 * mark reads more muted than its own stroke.
 *
 * Marker area is the band's share of the frame, and a band under the record's
 * `sparse_below_fraction` is drawn hollow. That matters: before the L* re-cut the
 * largest excursion on these charts rested on one pixel in fifteen million.
 */

const MARGINS = { left: 54, right: 78, top: 30, bottom: 52 };
const AXES: readonly { axis: CastAxis; label: string; hint: string; dash?: string }[] = [
  { axis: "a", label: "a*", hint: "green–red" },
  { axis: "b", label: "b*", hint: "blue–yellow", dash: "5 3" },
];

const styles = {
  // Scales into whatever box its card gives it. `viewBox` plus the default
  // `preserveAspectRatio` fits the drawing inside that box rather than
  // distorting it, so a capped card letterboxes the chart instead of stretching
  // it. `minHeight` is what lets a flex item shrink past its own content.
  svg: css.raw({
    display: "block",
    width: "full",
    height: "auto",
    minHeight: "zero",
    maxHeight: "full",
  }),
  line: css.raw({
    fill: "transparent",
    strokeWidth: "3px",
    strokeLinejoin: "round",
    strokeLinecap: "round",
  }),
  marker: css.raw({ stroke: "bg", strokeWidth: "1.5px" }),
  markerSparse: css.raw({ fill: "transparent", strokeWidth: "1.5px", strokeDasharray: "2 2" }),
  bandLabel: css.raw({ fill: "fg.dim", fontSize: "tick" }),
  seriesLabel: css.raw({ fill: "fg", fontFamily: "mono", fontSize: "key" }),
  seriesHint: css.raw({ fill: "fg.dim", fontSize: "tick" }),
};

/** Shorten a band name for an axis label: `deep_shadow` reads as `deep`. */
function shortBand(name: string): string {
  const [head, tail] = name.split("_");
  if (name === "above_diffuse_white") return "> wht";
  if (head === "low" || head === "high") return `${head}-${tail ?? ""}`;
  return head ?? name;
}

interface Props {
  cast: readonly CastBand[];
  /** Distinguishes the gradient ids when more than one chart is on a page. */
  id: string;
  /** The chart's `viewBox`; see `Histogram`'s note — stated, never defaulted. */
  width: number;
  height: number;
}

export function CastOverTone(props: Props) {
  /*
    Memoized for the same reason `Histogram` is — a plain `() => ...` recomputes
    on every read — though the shape of the waste here is its own. There are no
    bins: `y()`, `slots()` and `span()` are read once per band per axis and again
    per marker, and each read of `y()` re-entered `bounds()` -> `values()`, a
    `flatMap` rebuilding both means for every band. See `Histogram` for the
    measurement that covers both.
  */
  const width = createMemo(() => props.width);
  const height = createMemo(() => props.height);
  const plot = createMemo(() => plotArea(width(), height(), MARGINS));

  const values = createMemo(() => props.cast.flatMap((c) => [c.meanA, c.meanB]));
  // The axis covers every value on both curves, padded to a round number, and
  // always includes zero — the whole chart is read against neutral.
  const bounds = createMemo(() => paddedBounds(values()));
  // **The ramp is normalised by what was measured, never by the padded axis.**
  // Padding can push a frame whose worst band is 19 past the 20-unit reference,
  // and normalising by that would paint the same cast weaker on one frame than
  // another for no reason but axis geometry — the opposite of what the fixed
  // ramp exists to guarantee.
  const span = createMemo(() => rampSpan(...extent(values())));
  const step = createMemo(() => niceStep(bounds()[1] - bounds()[0]));

  // Bands the record omits carry no pixels, so they get no slot: the curve spans
  // the bands that exist rather than dipping to zero through a band that does not.
  const slots = createMemo(() => slotCentres(props.cast.length, plot().left, plot().right));
  const y = createMemo(() => linearScale(bounds(), [plot().top, plot().bottom], true));

  const valueOf = (band: CastBand, axis: CastAxis) => (axis === "a" ? band.meanA : band.meanB);

  /** Which of two fixed label rows this axis takes: whichever line ends higher. */
  const labelRow = (axis: CastAxis) => {
    const last = props.cast[props.cast.length - 1];
    const mine = last ? valueOf(last, axis) : 0;
    const other = last ? valueOf(last, axis === "a" ? "b" : "a") : 0;
    const top = plot().bottom - 62;
    // The tie is reachable, not theoretical: a neutral render gives
    // `mean_a == mean_b == 0` in every band, and `>=` on both axes would then
    // stack all four texts at one point. Break it on the axis name.
    return mine > other || (mine === other && axis === "a") ? top : top + 32;
  };

  return (
    <svg
      width={width()}
      height={height()}
      viewBox={`0 0 ${width()} ${height()}`}
      class={css(styles.svg)}
      role="img"
      aria-label="CIELAB a star and b star across the tone bands"
    >
      <defs>
        <For each={AXES}>
          {({ axis }) => (
            // Every read stays inside a function: `AXES` never changes identity,
            // so this row body runs once, and anything destructured here would
            // freeze at the first record while the scale kept moving.
            <linearGradient
              id={`${props.id}-${axis}`}
              gradientUnits="userSpaceOnUse"
              x1="0"
              y1={y()(bounds()[1])}
              x2="0"
              y2={y()(bounds()[0])}
            >
              <For each={rampGradientStops(axis, bounds()[0], bounds()[1], span())}>
                {(at) => <stop offset={at.offset} stop-color={at.color} />}
              </For>
            </linearGradient>
          )}
        </For>
      </defs>

      <Grid
        plot={plot()}
        orientation="horizontal"
        lines={ticks(bounds()[0], bounds()[1], step()).map((v) => ({
          value: v,
          at: y()(v),
          strong: v === 0,
        }))}
        format={(v) => (v > 0 ? `+${v}` : `${v}`)}
      />
      <For each={AXES}>
        {({ axis, label, hint, dash }) => (
          <>
            <polyline
              points={polyline(
                props.cast.map((band, i) => [slots()[i] ?? 0, y()(valueOf(band, axis))]),
              )}
              stroke={`url(#${props.id}-${axis})`}
              stroke-dasharray={dash}
              class={css(styles.line)}
            />
            <For each={props.cast}>
              {(band, i) => {
                const value = () => valueOf(band, axis);
                const colour = () => rampAt(axis, value(), span());
                return (
                  <Show
                    when={!band.sparse}
                    fallback={
                      <circle
                        cx={slots()[i()] ?? 0}
                        cy={y()(value())}
                        r={populationRadius(band.fraction, 4, 10)}
                        stroke={colour()}
                        class={css(styles.markerSparse)}
                      />
                    }
                  >
                    <circle
                      cx={slots()[i()] ?? 0}
                      cy={y()(value())}
                      r={populationRadius(band.fraction, 4, 10)}
                      fill={colour()}
                      class={css(styles.marker)}
                    />
                  </Show>
                );
              }}
            </For>
            {/* Direct label at the line's end, so identity never rests on colour.
                Stacked at fixed rows rather than tracking each line's last value:
                a near-neutral top band puts a* and b* within a couple of units of
                each other, and four texts would then overlap. The two rows keep
                the order the lines end in, so the pairing stays readable. */}
            <Show when={props.cast.length > 0}>
              <text x={plot().right + 12} y={labelRow(axis)} class={css(styles.seriesLabel)}>
                {label}
              </text>
              <text x={plot().right + 12} y={labelRow(axis) + 13} class={css(styles.seriesHint)}>
                {hint}
              </text>
            </Show>
          </>
        )}
      </For>

      <For each={props.cast}>
        {(band, i) => (
          <text
            x={slots()[i()] ?? 0}
            y={plot().bottom + 17}
            text-anchor="middle"
            class={css(styles.bandLabel)}
          >
            {shortBand(band.band)}
          </text>
        )}
      </For>

      <AxisLine plot={plot()} caption="tone band, dark to light" />
      <AxisTitle plot={plot()} text="CIELAB, 0 = neutral" />
    </svg>
  );
}
