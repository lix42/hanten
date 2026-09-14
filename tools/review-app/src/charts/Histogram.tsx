import { For, Show, createMemo } from "solid-js";
import { css } from "../../styled-system/css";
import { AxisLine, AxisTitle, Grid } from "./Frame";
import type { Histogram as HistogramData, SeriesName } from "./metrics";
import { histogramOutline, linearScale, niceStep, plotArea, ticks } from "./scale";

/**
 * Tone distribution, one bin per L* unit, as the record stores it.
 *
 * Serves both v1 histogram charts — the luminance view and the per-channel one
 * differ only in which series are drawn, so they are one component with a
 * `series` prop rather than two that would drift apart.
 *
 * **Channel colours carry a dash pattern as well as a hue.** Red and green are
 * indistinguishable under deuteranopia and no palette fixes that, so identity
 * rests on dash plus the direct label, with colour as the redundant cue.
 */

const MARGINS = { left: 52, right: 16, top: 24, bottom: 46 };

/** Dash patterns, the secondary encoding that carries identity when hue cannot. */
const DASH: Readonly<Record<SeriesName, string | undefined>> = {
  luminance: undefined,
  r: "6 3",
  g: "2 3",
  b: "5 2 1 2",
};

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
  fill: css.raw({ fillOpacity: 0.13, strokeWidth: "1.5px", strokeLinejoin: "round" }),
  reference: css.raw({ stroke: "accent", strokeWidth: "1.5px", strokeDasharray: "4 3" }),
  referenceLabel: css.raw({ fill: "accent", fontSize: "tick" }),
  legendLabel: css.raw({ fill: "fg", fontFamily: "mono", fontSize: "tick" }),
  midGrey: css.raw({ stroke: "gridStrong", strokeWidth: "1px" }),
  clipped: css.raw({ fill: "bad", fontSize: "tick" }),
  // Stroke is for the outline; the legend text takes the fill-only pair, or a
  // 1px default stroke outlines every 10px glyph and fills in its counters.
  seriesLuminance: css.raw({ stroke: "series.luminance", fill: "series.luminance" }),
  seriesR: css.raw({ stroke: "series.r", fill: "series.r" }),
  seriesG: css.raw({ stroke: "series.g", fill: "series.g" }),
  seriesB: css.raw({ stroke: "series.b", fill: "series.b" }),
  inkLuminance: css.raw({ fill: "series.luminance" }),
  inkR: css.raw({ fill: "series.r" }),
  inkG: css.raw({ fill: "series.g" }),
  inkB: css.raw({ fill: "series.b" }),
};

/** Token-backed colour per channel. */
function seriesStyle(name: SeriesName) {
  if (name === "r") return styles.seriesR;
  if (name === "g") return styles.seriesG;
  if (name === "b") return styles.seriesB;
  return styles.seriesLuminance;
}

/** The same colour as a fill only, for text. */
function seriesInk(name: SeriesName) {
  if (name === "r") return styles.inkR;
  if (name === "g") return styles.inkG;
  if (name === "b") return styles.inkB;
  return styles.inkLuminance;
}

interface Props {
  histogram: HistogramData;
  /** Which channels to draw, in order. */
  series: readonly SeriesName[];
  /** How far up the L* axis to show. The record stores 200; a render rarely passes 110. */
  visibleLstar?: number;
  /**
   * The chart's `viewBox`, stated by the caller rather than defaulted.
   *
   * Required because the three charts in a panel must share one box to render at
   * the same size, and per-component defaults are exactly how they drifted apart
   * before — this one defaulted to 300 tall and the cast chart to 330.
   */
  width: number;
  height: number;
}

export function Histogram(props: Props) {
  /*
    Every derivation below is a `createMemo`, and that is load-bearing rather
    than tidiness. A plain `() => ...` is recomputed on **every read**, and the
    scales here are read once per bin by `histogramOutline`'s callbacks — so each
    of ~220 points re-entered `y()` -> `ceiling()` -> `peak()` -> `drawn()`, a
    full scan of every series' bins. Measured: one config switch cost ~360ms of
    synchronous work across three mounted frames, of which the charts were 99%
    (the same set with its measurements removed switched in 2ms).
  */
  const width = createMemo(() => props.width);
  const height = createMemo(() => props.height);
  const visible = createMemo(() => props.visibleLstar ?? 110);
  const plot = createMemo(() => plotArea(width(), height(), MARGINS));

  /*
    **Declared before `drawn`/`peak`, and that ordering is now required.** A
    `createMemo` body runs eagerly at creation, where a plain `() => ...` ran
    only when first read — so a memo that calls a `const` declared further down
    hits the temporal dead zone instead of resolving later. `peak` calls
    `visibleBins`, and memoizing in place turned that into a `ReferenceError`
    that blanked the whole page.
  */
  /**
   * Bin index -> L*, taken from the record rather than assumed.
   *
   * One bin per L* unit is true today, and equating the two would work — until a
   * producer re-cuts the histogram, as it already re-cut the bands once. Then the
   * axis would silently mislabel with nothing to catch it.
   */
  const binWidth = createMemo(() => {
    const [lo, hi] = props.histogram.lstarRange;
    return props.histogram.bins === 0 ? 1 : (hi - lo) / props.histogram.bins;
  });
  const lstarOf = (bin: number) => props.histogram.lstarRange[0] + bin * binWidth();
  const visibleBins = createMemo(() =>
    Math.ceil((visible() - props.histogram.lstarRange[0]) / binWidth()),
  );
  // A requested channel the record does not carry is refused, not dropped: a chart
  // silently missing one curve looks exactly like a frame whose channel is flat.
  // `parseMetrics` already refuses such a record, which is where the failure
  // belongs — a throw *here* is a render error that replaces the whole page
  // instead of one rendition's charts. This stays as the guard for a model built
  // by hand.
  const drawn = createMemo(() =>
    props.series.map((name) => {
      const series = props.histogram.series[name];
      if (!series) {
        const have = Object.keys(props.histogram.series).join(", ") || "no series at all";
        throw new Error(`tone.histogram.series has no ${JSON.stringify(name)}; it carries ${have}`);
      }
      return { name, series };
    }),
  );

  /** The tallest bin in view, so one frame's peak fills the plot. */
  const peak = createMemo(() => {
    const total = props.histogram.pixels || 1;
    let max = 0;
    for (const { series } of drawn()) {
      for (let i = 0; i < Math.min(series.counts.length, visibleBins()); i += 1) {
        max = Math.max(max, (series.counts[i] ?? 0) / total);
      }
    }
    return max;
  });
  // Round up to a whole percent so the gridlines land on readable numbers.
  const ceiling = createMemo(() => Math.max(Math.ceil(peak() * 100), 1) / 100);
  const step = createMemo(() => niceStep(ceiling()));

  /** Share of the frame the drawn range leaves out, counting `above_range`. */
  const clipped = createMemo(() => {
    const total = props.histogram.pixels || 1;
    let out = 0;
    for (const { series } of drawn()) {
      let beyond = series.aboveRange;
      for (let i = visibleBins(); i < series.counts.length; i += 1) beyond += series.counts[i] ?? 0;
      out = Math.max(out, beyond / total);
    }
    return out;
  });

  const x = createMemo(() =>
    linearScale([props.histogram.lstarRange[0], visible()], [plot().left, plot().right]),
  );
  const y = createMemo(() => linearScale([0, ceiling()], [plot().top, plot().bottom], true));

  return (
    <svg
      width={width()}
      height={height()}
      viewBox={`0 0 ${width()} ${height()}`}
      class={css(styles.svg)}
      role="img"
      aria-label={`Tone distribution in CIELAB lightness, ${props.series.join(", ")}`}
    >
      <Grid
        plot={plot()}
        orientation="horizontal"
        lines={ticks(0, ceiling(), step()).map((v) => ({
          value: v,
          at: y()(v),
        }))}
        format={(v) => `${(v * 100).toFixed(step() < 0.01 ? 1 : 0)}%`}
      />
      <Grid
        plot={plot()}
        orientation="vertical"
        lines={ticks(props.histogram.lstarRange[0], visible(), 10).map((v) => ({
          value: v,
          at: x()(v),
        }))}
        format={(v) => `${v}`}
      />

      {/* The two reference lines the record names, so the L* curve is never
          re-derived here. Mid grey (bin 49) falls between ticks and could not be
          carried by one. Diffuse white is different: it lands on L* 100 exactly,
          because the producer puts it on the bin-100 boundary, so the accent line
          is drawn *over* that gridline rather than instead of it. Dropping the
          tick would take its "100" label with it and leave the axis reading
          90 then 110. */}
      <Show when={lstarOf(props.histogram.midGreyBin) <= visible()}>
        <line
          x1={x()(lstarOf(props.histogram.midGreyBin))}
          y1={plot().top}
          x2={x()(lstarOf(props.histogram.midGreyBin))}
          y2={plot().bottom}
          class={css(styles.midGrey)}
        />
      </Show>

      {/* Diffuse white: the ceiling a render either reaches or stops short of.
          Its bin is named by the record, so the L* curve is never re-derived. */}
      <Show when={lstarOf(props.histogram.diffuseWhiteBin) <= visible()}>
        <line
          x1={x()(lstarOf(props.histogram.diffuseWhiteBin))}
          y1={plot().top}
          x2={x()(lstarOf(props.histogram.diffuseWhiteBin))}
          y2={plot().bottom}
          class={css(styles.reference)}
        />
        <text
          x={x()(lstarOf(props.histogram.diffuseWhiteBin)) - 5}
          y={plot().top + 10}
          text-anchor="end"
          class={css(styles.referenceLabel)}
        >
          diffuse white
        </text>
      </Show>

      <For each={drawn()}>
        {({ name, series }) => (
          <polyline
            points={histogramOutline(
              series.counts,
              (bin) => x()(lstarOf(bin)),
              (share) => y()(share),
              props.histogram.pixels,
              visibleBins(),
            )}
            stroke-dasharray={DASH[name]}
            class={css(styles.fill, seriesStyle(name))}
          />
        )}
      </For>

      {/* Direct labels, because dash and hue alone cannot carry identity here. */}
      <For each={drawn()}>
        {({ name }, index) => (
          <text
            x={plot().left + 6}
            y={plot().top + 12 + index() * 13}
            class={css(styles.legendLabel, seriesInk(name))}
          >
            {name}
          </text>
        )}
      </For>

      {/* Anything past the drawn range, said out loud. The record runs to twice
          diffuse white precisely so an HDR rendition's headroom is measurable —
          nc's own 1000/203 ceiling is L* 181 — and clipping the axis without
          saying so would show a truncated distribution as if it were whole. */}
      <Show when={clipped() > 0}>
        <text
          x={plot().right - 4}
          y={plot().top + 26}
          text-anchor="end"
          class={css(styles.clipped)}
        >
          {`${(clipped() * 100).toFixed(2)}% above L* ${visible()}`}
        </text>
      </Show>

      <AxisLine plot={plot()} caption="CIELAB L*, as the record stores it" />
      <AxisTitle plot={plot()} text="share of region" />
    </svg>
  );
}
