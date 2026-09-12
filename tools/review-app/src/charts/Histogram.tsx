import { For, Show } from "solid-js";
import { css } from "../../styled-system/css";
import { AxisLine, AxisTitle, Grid } from "./Frame";
import type { Histogram as HistogramData } from "./metrics";
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
const DASH: Readonly<Record<string, string | undefined>> = {
  luminance: undefined,
  r: "6 3",
  g: "2 3",
  b: "5 2 1 2",
};

const styles = {
  svg: css.raw({ display: "block", width: "full", height: "auto" }),
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

/** Token-backed colour per channel. A name with no entry falls back to luminance. */
function seriesStyle(name: string) {
  if (name === "r") return styles.seriesR;
  if (name === "g") return styles.seriesG;
  if (name === "b") return styles.seriesB;
  return styles.seriesLuminance;
}

/** The same colour as a fill only, for text. */
function seriesInk(name: string) {
  if (name === "r") return styles.inkR;
  if (name === "g") return styles.inkG;
  if (name === "b") return styles.inkB;
  return styles.inkLuminance;
}

interface Props {
  histogram: HistogramData;
  /** Which channels to draw, in order. */
  series: readonly string[];
  /** How far up the L* axis to show. The record stores 200; a render rarely passes 110. */
  visibleLstar?: number;
  width?: number;
  height?: number;
}

export function Histogram(props: Props) {
  const width = () => props.width ?? 500;
  const height = () => props.height ?? 300;
  const visible = () => props.visibleLstar ?? 110;
  const plot = () => plotArea(width(), height(), MARGINS);

  const drawn = () =>
    props.series.flatMap((name) => {
      const series = props.histogram.series[name];
      return series ? [{ name, series }] : [];
    });

  /** The tallest bin in view, so one frame's peak fills the plot. */
  const peak = () => {
    const total = props.histogram.pixels || 1;
    let max = 0;
    for (const { series } of drawn()) {
      for (let i = 0; i < Math.min(series.counts.length, visibleBins()); i += 1) {
        max = Math.max(max, (series.counts[i] ?? 0) / total);
      }
    }
    return max;
  };
  // Round up to a whole percent so the gridlines land on readable numbers.
  const ceiling = () => Math.max(Math.ceil(peak() * 100), 1) / 100;
  const step = () => niceStep(ceiling());

  /**
   * Bin index -> L*, taken from the record rather than assumed.
   *
   * One bin per L* unit is true today, and equating the two would work — until a
   * producer re-cuts the histogram, as it already re-cut the bands once. Then the
   * axis would silently mislabel with nothing to catch it.
   */
  const binWidth = () => {
    const [lo, hi] = props.histogram.lstarRange;
    return props.histogram.bins === 0 ? 1 : (hi - lo) / props.histogram.bins;
  };
  const lstarOf = (bin: number) => props.histogram.lstarRange[0] + bin * binWidth();
  const visibleBins = () => Math.ceil((visible() - props.histogram.lstarRange[0]) / binWidth());
  /** Share of the frame the drawn range leaves out, counting `above_range`. */
  const clipped = () => {
    const total = props.histogram.pixels || 1;
    let out = 0;
    for (const { series } of drawn()) {
      let beyond = series.aboveRange;
      for (let i = visibleBins(); i < series.counts.length; i += 1) beyond += series.counts[i] ?? 0;
      out = Math.max(out, beyond / total);
    }
    return out;
  };

  const x = () =>
    linearScale([props.histogram.lstarRange[0], visible()], [plot().left, plot().right]);
  const y = () => linearScale([0, ceiling()], [plot().top, plot().bottom], true);

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
