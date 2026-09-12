/**
 * Chart geometry, kept free of the DOM so it can be tested directly.
 *
 * The components are thin on purpose: every number a chart draws is computed
 * here, because `vite.config.ts` collects only `.test.ts` files under `src/` and
 * runs them with `environment: "node"` — so nothing inside a `.tsx` file can be
 * covered at all. The mini-map in `ImageSection.tsx` is the counter-example — its scale math
 * sits inline in the component and has never been tested.
 */

/** The drawable rectangle inside a chart, after margins. */
export interface Plot {
  readonly left: number;
  readonly right: number;
  readonly top: number;
  readonly bottom: number;
}

export interface Margins {
  readonly left: number;
  readonly right: number;
  readonly top: number;
  readonly bottom: number;
}

export function plotArea(width: number, height: number, margins: Margins): Plot {
  return {
    left: margins.left,
    right: width - margins.right,
    top: margins.top,
    bottom: height - margins.bottom,
  };
}

/**
 * A linear map from a data range onto a pixel range.
 *
 * `flip` is for the y axis, where larger values sit at smaller pixel coordinates.
 * A zero-width domain maps everything to the low end rather than dividing by
 * zero — a chart of one constant value should draw a flat line, not `NaN`.
 */
export function linearScale(
  domain: readonly [number, number],
  range: readonly [number, number],
  flip = false,
): (value: number) => number {
  const [d0, d1] = domain;
  const [r0, r1] = flip ? [range[1], range[0]] : range;
  const span = d1 - d0;
  return (value) => (span === 0 ? r0 : r0 + ((value - d0) * (r1 - r0)) / span);
}

/**
 * Evenly spaced slot centres, for a categorical axis such as the tone bands.
 *
 * The first and last slots sit *on* the plot edges rather than inset, so a curve
 * spans the full width. One slot is centred.
 */
export function slotCentres(count: number, left: number, right: number): number[] {
  if (count <= 0) return [];
  if (count === 1) return [(left + right) / 2];
  return Array.from({ length: count }, (_, i) => left + (i * (right - left)) / (count - 1));
}

/**
 * Tick values at a round step covering `[lo, hi]`, always including zero when it
 * falls inside — the neutral line is the one a cast chart is read against.
 */
export function ticks(lo: number, hi: number, step: number): number[] {
  if (step <= 0) return [];
  const out: number[] = [];
  for (let t = Math.ceil(lo / step) * step; t <= hi + 1e-9; t += step) {
    // Re-round each step: repeated addition of a fractional step drifts.
    out.push(Math.abs(t) < 1e-9 ? 0 : Number((Math.round(t / step) * step).toFixed(6)));
  }
  return out;
}

/** `x,y` pairs for an SVG `points` attribute. */
export function polyline(points: readonly (readonly [number, number])[]): string {
  return points.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
}

/**
 * A histogram outline: one horizontal run per bin, closed down to the baseline
 * at both ends so the shape can be filled.
 *
 * Drawn as a step rather than a line through bin centres because a bin covers an
 * interval — joining centres would imply the count tapers between them, and at
 * one bin per L* unit the difference is visible at the spikes.
 */
export function histogramOutline(
  counts: readonly number[],
  x: (bin: number) => number,
  y: (value: number) => number,
  total: number,
  visibleBins: number,
): string {
  const baseline = y(0);
  const shown = Math.min(counts.length, visibleBins);
  const points: (readonly [number, number])[] = [[x(0), baseline]];
  for (let i = 0; i < shown; i += 1) {
    const share = total === 0 ? 0 : (counts[i] ?? 0) / total;
    points.push([x(i), y(share)], [x(i + 1), y(share)]);
  }
  points.push([x(shown), baseline]);
  return polyline(points);
}

/** Marker radius that encodes a band's share of the frame, with a visible floor. */
export function populationRadius(fraction: number, floor: number, gain: number): number {
  return floor + gain * Math.sqrt(Math.max(fraction, 0));
}

/**
 * A round gridline step giving roughly `target` lines across `[0, hi]`.
 *
 * A fixed step cannot serve both ends of the range this chart sees: a typical
 * frame's peak bin holds about 3% of the pixels, while a near-black render — a
 * documented nc failure mode, and one worth charting — approaches 100%. At a
 * fixed 2% the second would label fifty-one lines across a 230px plot.
 */
export function niceStep(hi: number, target = 6): number {
  if (hi <= 0 || target <= 0) return 1;
  const rough = hi / target;
  const magnitude = 10 ** Math.floor(Math.log10(rough));
  return ([1, 2, 5, 10].find((m) => m * magnitude >= rough) ?? 10) * magnitude;
}
