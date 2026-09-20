/**
 * Where the picture actually is, and how a point on it maps to the image.
 *
 * Both new overlays — patches and the colour readout — need one answer: given
 * the `<img>` element's box, which part of it is *painted*, and what image pixel
 * sits under a given client point. In `fullsize` those are the same box; in
 * `fit` the picture is letterboxed inside the element by `object-fit: contain`,
 * so a patch drawn against the element box would sit off the subject by however
 * much letterboxing there is — on a portrait frame in a landscape pane, most of
 * the width.
 *
 * Patches are stored **normalised to the image**, not to the pane, which is what
 * lets one rectangle survive a fit/fullsize toggle, a window resize and a switch
 * to a config that renders at a different size.
 *
 * Pure, and tested: `.tsx` is not collected by the runner, and this is the
 * arithmetic that decides whether an overlay lands on the right pixels.
 */

import type { ZoomMode } from "./review";

export interface Point {
  readonly x: number;
  readonly y: number;
}

export interface Box {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

/** A rectangle in image space: every field a fraction of the image, 0..1. */
export interface NormalRect {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

/**
 * The box `object-fit: contain` with `object-position: center` paints into.
 *
 * A degenerate natural size (an image that has not decoded yet) leaves the box
 * alone rather than dividing by zero — the caller then simply measures again on
 * load, which it does anyway.
 */
export function containedBox(box: Box, naturalWidth: number, naturalHeight: number): Box {
  if (naturalWidth <= 0 || naturalHeight <= 0 || box.width <= 0 || box.height <= 0) return box;
  const scale = Math.min(box.width / naturalWidth, box.height / naturalHeight);
  const width = naturalWidth * scale;
  const height = naturalHeight * scale;
  return {
    x: box.x + (box.width - width) / 2,
    y: box.y + (box.height - height) / 2,
    width,
    height,
  };
}

/**
 * The painted box for either zoom mode.
 *
 * `fullsize` renders at natural size with nothing to letterbox, so the element
 * box *is* the picture; `fit` contains it. One function so no call site has to
 * remember which mode letterboxes.
 */
export function paintedBox(
  box: Box,
  naturalWidth: number,
  naturalHeight: number,
  zoom: ZoomMode,
): Box {
  return zoom === "fit" ? containedBox(box, naturalWidth, naturalHeight) : box;
}

function clamp01(value: number): number {
  return Math.max(0, Math.min(1, value));
}

/** Where a point sits inside the painted picture, 0..1, clamped to it. */
export function normalizePoint(point: Point, painted: Box): Point {
  if (painted.width <= 0 || painted.height <= 0) return { x: 0, y: 0 };
  return {
    x: clamp01((point.x - painted.x) / painted.width),
    y: clamp01((point.y - painted.y) / painted.height),
  };
}

/**
 * The rectangle a drag from `a` to `b` describes, in image space.
 *
 * Normalised from the two corners rather than from a start plus a signed size,
 * so dragging up and to the left is the same gesture as dragging down and to the
 * right — there is no wrong direction to select a patch in.
 */
export function rectFromDrag(a: Point, b: Point, painted: Box): NormalRect {
  const first = normalizePoint(a, painted);
  const second = normalizePoint(b, painted);
  return {
    x: Math.min(first.x, second.x),
    y: Math.min(first.y, second.y),
    width: Math.abs(second.x - first.x),
    height: Math.abs(second.y - first.y),
  };
}

/** Back to pixels inside the painted box, for drawing a stored patch. */
export function rectToBox(rect: NormalRect, painted: Box): Box {
  return {
    x: painted.x + rect.x * painted.width,
    y: painted.y + rect.y * painted.height,
    width: rect.width * painted.width,
    height: rect.height * painted.height,
  };
}

/**
 * The smallest drag that counts as a patch, in CSS pixels on each axis.
 *
 * Below it the gesture was a click — which in patch mode means "I missed", not
 * "select nothing". Without a floor, every stray click opens the label dialog
 * for a zero-area rectangle.
 */
export const MIN_PATCH_PX = 8;

/**
 * Whether a drag describes a patch worth keeping.
 *
 * **Measured on the rectangle that would be *kept*, not on the raw endpoints.**
 * `rectFromDrag` clamps both corners into the picture, so a drag that starts on
 * an edge and runs outward covers plenty of screen while collapsing to a sliver
 * — at the very edge, to nothing at all. Testing the raw drag let that through:
 * the dialog opened, and labelling it produced a patch that drew as a line and
 * copied as `w 0.0%`.
 */
export function isDragWorthKeeping(a: Point, b: Point, painted: Box): boolean {
  return (
    clampedSpan(a.x, b.x, painted.x, painted.width) >= MIN_PATCH_PX &&
    clampedSpan(a.y, b.y, painted.y, painted.height) >= MIN_PATCH_PX
  );
}

/**
 * How much of one axis survives clamping into the picture, in pixels.
 *
 * Measured directly rather than by normalising and multiplying back: that round
 * trip divides and re-multiplies by the same extent, and the rounding leaves a
 * drag of exactly `MIN_PATCH_PX` one ulp short of its own threshold — so the
 * floor rejected the smallest drag it is meant to accept.
 */
function clampedSpan(from: number, to: number, start: number, extent: number): number {
  const end = start + extent;
  const lo = Math.min(Math.max(Math.min(from, to), start), end);
  const hi = Math.min(Math.max(Math.max(from, to), start), end);
  return hi - lo;
}

/**
 * The image pixel under a point, and how many image pixels one screen pixel covers.
 *
 * The span is what makes the readout honest in `fit`. There, one screen pixel is
 * several image pixels wide, and reading a single one of them reports grain
 * rather than the colour you are looking at — so the sampler asks the browser to
 * downscale exactly that span to one pixel, the same reduction the browser
 * performed to draw it. In `fullsize` on an ordinary display the span is 1 and
 * the reading is the file's own pixel.
 *
 * **`devicePixelRatio` is part of that, and leaving it out is a 4x error in
 * area on a 2x display.** A raster image is rasterised at *device* resolution,
 * so what the screen shows is `painted.width * ratio` samples across, not
 * `painted.width`. Dividing by CSS pixels alone averages a box twice as wide as
 * the one the browser drew, which shows wherever the spot has detail — a thin
 * specular highlight, a grain edge. It is passed in rather than read from
 * `window` so this stays pure and testable; at ratio 1 it changes nothing.
 */
export function samplePlan(
  point: Point,
  painted: Box,
  naturalWidth: number,
  naturalHeight: number,
  devicePixelRatio: number,
): { x: number; y: number; span: number } | undefined {
  if (painted.width <= 0 || painted.height <= 0) return undefined;
  if (naturalWidth <= 0 || naturalHeight <= 0) return undefined;
  if (
    point.x < painted.x ||
    point.y < painted.y ||
    point.x > painted.x + painted.width ||
    point.y > painted.y + painted.height
  ) {
    return undefined;
  }
  const ratio = devicePixelRatio > 0 ? devicePixelRatio : 1;
  const span = Math.max(1, Math.round(naturalWidth / (painted.width * ratio)));
  const scaled = normalizePoint(point, painted);
  // Clamped to the last whole sample box, so a point on the right or bottom edge
  // still reads pixels that exist.
  const x = Math.min(Math.floor(scaled.x * naturalWidth), naturalWidth - span);
  const y = Math.min(Math.floor(scaled.y * naturalHeight), naturalHeight - span);
  return { x: Math.max(0, x), y: Math.max(0, y), span };
}

/**
 * Where the colour readout goes: beside the cursor, flipped at the far edges.
 *
 * Offset rather than centred on the point, because the point is the thing being
 * read — a chip under the cursor would cover the pixel it describes. Flipping
 * (rather than clamping) keeps that true near an edge: sliding the chip back
 * along the pane would slide it over the cursor.
 */
export function placeReadout(
  point: Point,
  pane: { width: number; height: number },
  chip: { width: number; height: number },
  gap: number,
): { left: number; top: number } {
  const fitsRight = point.x + gap + chip.width <= pane.width;
  const fitsBelow = point.y + gap + chip.height <= pane.height;
  return {
    left: Math.max(0, fitsRight ? point.x + gap : point.x - gap - chip.width),
    top: Math.max(0, fitsBelow ? point.y + gap : point.y - gap - chip.height),
  };
}

/**
 * Whether a patch's label chip must sit *inside* the rectangle.
 *
 * The chip is drawn above the rectangle, and the scrolling viewport around the
 * overlay clips anything at a negative offset — so a patch against the top of
 * the picture would show as a bare rectangle with no label, which is the one
 * thing a patch must not be.
 */
export function chipGoesInside(box: Box, chipHeight: number): boolean {
  return box.y < chipHeight;
}
