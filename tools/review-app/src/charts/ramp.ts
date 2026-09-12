/**
 * The colour ramps that make the cast chart readable, kept free of the DOM so
 * they can be tested directly.
 *
 * `cast_by_tone_band` reports two CIELAB numbers per tone band, and nobody
 * remembers which way they point. So each line is **coloured by its own value**:
 * `a*` runs green below zero to red above, `b*` blue below to yellow above. The
 * line teaches its own axis.
 *
 * Two properties this module exists to hold:
 *
 * 1. **Colour is redundant with position.** The ramp is fixed, never scaled to
 *    the data — scale it and a mild cast on one frame looks like a severe one on
 *    another.
 * 2. **Each axis ends where sRGB runs out, and the two ends differ.** Measured at
 *    `RAMP_LIGHTNESS`: green clips at 42.8 and blue at 54.3. A symmetric ramp can
 *    only reach its tightest end, so `a*` caps at 41 and `b*` at 52. One number
 *    for both would under-saturate `b*`; a larger one skews hue instead of
 *    saturating, because out-of-gamut components clamp per channel.
 */

/** The lightness every swatch is drawn at. The job is hue, not appearance. */
export const RAMP_LIGHTNESS = 65;

/** Ramp endpoints, just inside the sRGB gamut at `RAMP_LIGHTNESS`. */
export const RAMP_CHROMA = { a: 41, b: 52 } as const;

export type CastAxis = keyof typeof RAMP_CHROMA;

// D65 as nc's own colorimetry derives it — from `definitions::D65`'s rounded
// chromaticity, not the widely tabulated triple. `scripts/analysis/nctool/
// metrics.py` says the same thing at more length; the two must agree or a cast
// would be painted a different colour than it was measured as.
const WHITE: readonly [number, number, number] = [0.950456, 1.0, 1.089058];

const XYZ_TO_LINEAR_SRGB: readonly (readonly [number, number, number])[] = [
  [3.2404542, -1.5371385, -0.4985314],
  [-0.969266, 1.8760108, 0.041556],
  [0.0556434, -0.2040259, 1.0572252],
];

function labToLinearSrgb(l: number, a: number, b: number): [number, number, number] {
  const fy = (l + 16) / 116;
  const f = (t: number) => (t ** 3 > 0.008856 ? t ** 3 : (116 * t - 16) / 903.3);
  const xyz = [f(fy + a / 500) * WHITE[0], f(fy) * WHITE[1], f(fy - b / 200) * WHITE[2]] as const;
  return XYZ_TO_LINEAR_SRGB.map((row) => row.reduce((sum, k, i) => sum + k * (xyz[i] ?? 0), 0)) as [
    number,
    number,
    number,
  ];
}

/** Whether a CIELAB colour survives the trip to sRGB without a channel clamping. */
export function inSrgbGamut(l: number, a: number, b: number): boolean {
  const eps = 1e-6;
  return labToLinearSrgb(l, a, b).every((c) => c >= -eps && c <= 1 + eps);
}

function hex(l: number, a: number, b: number): string {
  const encode = (c: number) =>
    c <= 0.0031308 ? 12.92 * c : 1.055 * Math.abs(c) ** (1 / 2.4) - 0.055;
  const byte = (c: number) =>
    Math.max(0, Math.min(255, Math.round(encode(c) * 255)))
      .toString(16)
      .padStart(2, "0");
  const [r, g, bl] = labToLinearSrgb(l, a, b);
  return `#${byte(r)}${byte(g)}${byte(bl)}`;
}

/** The colour of a point on one axis's ramp, at chroma `c` from neutral. */
export function rampColour(axis: CastAxis, c: number): string {
  return axis === "a" ? hex(RAMP_LIGHTNESS, c, 0) : hex(RAMP_LIGHTNESS, 0, c);
}

/**
 * The CIELAB magnitude at which a ramp reaches full chroma.
 *
 * Chosen against real records: measured casts run to about `b* = 20` on the
 * frames this was designed with, so the reference covers the observed range
 * while leaving a mild cast visibly mild.
 */
export const RAMP_REFERENCE = 20;

/**
 * The span a chart normalises by, given the values it is plotting.
 *
 * Fixed at `RAMP_REFERENCE` so frames paint comparably, and widened only when a
 * frame exceeds it — which keeps the hue honest rather than clamping a whole
 * band to full chroma.
 */
export function rampSpan(lo: number, hi: number): number {
  return Math.max(RAMP_REFERENCE, Math.abs(lo), Math.abs(hi));
}

/**
 * The colour for `value`, normalised by `span` and clamped at full chroma.
 *
 * The *same* function fills a marker and builds the gradient stop under it. The
 * prototype computed the two separately — the marker used the value's true
 * colour while the line spanned the axis — and a marker read visibly more muted
 * than its own stroke, worst exactly where the cast is strongest.
 */
export function rampAt(axis: CastAxis, value: number, span: number): string {
  if (span === 0) return rampColour(axis, 0);
  const unit = Math.max(-1, Math.min(1, value / span));
  return rampColour(axis, unit * RAMP_CHROMA[axis]);
}

/**
 * The stops of the vertical gradient that paints one curve, top of the axis
 * first.
 *
 * Colour has to be the value's colour at every y, and `rampAt` is linear only
 * inside `±span`. So an axis padded past the reference gets an extra stop *at*
 * the reference: without it the interpolation stretches the whole ramp over the
 * padding and paints a measured value short of the chroma it earned.
 */
export function rampGradientStops(
  axis: CastAxis,
  lo: number,
  hi: number,
  span: number,
): { offset: string; color: string }[] {
  const inside = [hi, span, 0, -span, lo].filter((v) => v >= lo && v <= hi);
  return [...new Set(inside)]
    .sort((x, y) => y - x)
    .map((v) => ({
      offset: hi === lo ? "0.5" : ((hi - v) / (hi - lo)).toFixed(4),
      color: rampAt(axis, v, span),
    }));
}
