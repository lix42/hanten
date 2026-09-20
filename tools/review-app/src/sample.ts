/**
 * Reading one pixel back off a rendition.
 *
 * Touches the DOM, so the arithmetic it needs lives in `geometry.ts` and is
 * tested there; what is left here is the canvas call itself.
 *
 * **The canvas is 1x1 and the crop is done by `drawImage`.** A natural-size
 * canvas of a 5184x3600 scan is ~75 MB, per rendition, and the app already keeps
 * six of them mounted — so nothing is cached and nothing large is allocated:
 * each sample asks the browser to downscale the span of image pixels under the
 * cursor into a single pixel — the same reduction the browser performed to draw
 * it, `devicePixelRatio` included (see `samplePlan`). In `fit` that span is
 * several pixels wide; in `fullsize` on an ordinary display it is one pixel and
 * the reading is the file's own.
 *
 * The values come back as **sRGB as displayed**: a 2D canvas colour-manages the
 * source into its own colour space, which is sRGB unless asked otherwise, so a
 * Display P3 rendition reads as the screen shows it and its out-of-sRGB colours
 * read clipped. That is the deliberate choice — the readout answers "what colour
 * am I looking at", not "what did nc encode".
 */

import type { Rgb } from "./color";

/**
 * One reusable context, because a fresh canvas per pointer move is a fresh GPU
 * allocation per pointer move. `willReadFrequently` moves it to a software
 * surface, which is what `getImageData` wants — the whole point of this one.
 */
let context: CanvasRenderingContext2D | null | undefined;

function sampler(): CanvasRenderingContext2D | undefined {
  if (context === undefined) {
    const canvas = document.createElement("canvas");
    canvas.width = 1;
    canvas.height = 1;
    context = canvas.getContext("2d", { willReadFrequently: true });
  }
  return context ?? undefined;
}

/**
 * The colour of the `span`x`span` image pixels at (`x`, `y`), or `undefined`.
 *
 * `undefined` rather than a guess whenever the browser will not answer: a
 * rendition still decoding, a canvas this build has no 2D context for, or —
 * were an image ever served cross-origin — a tainted canvas, which throws on
 * `getImageData`. A readout that invented a colour would be worse than one that
 * says nothing, on a page whose whole job is judging colour.
 */
export function sampleImage(
  image: HTMLImageElement,
  x: number,
  y: number,
  span: number,
): Rgb | undefined {
  if (!image.complete || image.naturalWidth === 0) return undefined;
  const ctx = sampler();
  if (!ctx) return undefined;
  try {
    ctx.clearRect(0, 0, 1, 1);
    ctx.drawImage(image, x, y, span, span, 0, 0, 1, 1);
    const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data;
    if (r === undefined || g === undefined || b === undefined) return undefined;
    // Nothing nc writes is transparent, but a fully transparent pixel would
    // otherwise read as black — a real colour, and the wrong one.
    if (a === 0) return undefined;
    return { r, g, b };
  } catch {
    return undefined;
  }
}
