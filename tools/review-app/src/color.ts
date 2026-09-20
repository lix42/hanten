/**
 * What colour a pixel is, in words and in numbers.
 *
 * The problem this solves is stated by the person who asked for it: two renders
 * are visibly different but the *direction* of the difference is not readable by
 * eye at small magnitudes. A HEX pair answers that where an eye cannot.
 *
 * Every value here is **sRGB as displayed**. The sampler draws through a plain
 * sRGB canvas, so the browser has already converted from whatever profile the
 * rendition carries — which is the colour on the screen you are judging, not the
 * encoded value nc wrote. A Display P3 rendition's out-of-sRGB colours therefore
 * read clipped, and that is the documented trade.
 *
 * Kept free of the DOM so it can be tested directly — `.tsx` is not collected by
 * the test runner, so anything with an answer lives in a `.ts` like this one.
 */

/** A sampled pixel: 8-bit sRGB, the range `getImageData` returns. */
export interface Rgb {
  readonly r: number;
  readonly g: number;
  readonly b: number;
}

/** Hue in degrees, saturation and lightness as fractions. */
export interface Hsl {
  readonly h: number;
  readonly s: number;
  readonly l: number;
}

function clampByte(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(255, Math.round(value)));
}

/** `#RRGGBB`, upper case — the spelling a HEX field elsewhere expects to be given. */
export function toHex(rgb: Rgb): string {
  const part = (value: number) => clampByte(value).toString(16).padStart(2, "0").toUpperCase();
  return `#${part(rgb.r)}${part(rgb.g)}${part(rgb.b)}`;
}

export function rgbToHsl(rgb: Rgb): Hsl {
  const r = clampByte(rgb.r) / 255;
  const g = clampByte(rgb.g) / 255;
  const b = clampByte(rgb.b) / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const span = max - min;
  if (span === 0) return { h: 0, s: 0, l };
  // The usual HSL saturation: the span, normalised by how much room a colour of
  // this lightness has to be saturated at all.
  const s = span / (1 - Math.abs(2 * l - 1));
  let h: number;
  if (max === r) h = ((g - b) / span) % 6;
  else if (max === g) h = (b - r) / span + 2;
  else h = (r - g) / span + 4;
  h *= 60;
  return { h: h < 0 ? h + 360 : h, s, l };
}

/**
 * CIE L*, 0–100 — the lightness the readout shows.
 *
 * Not HSL's `l`. That one is `(max + min) / 2` over the *encoded* channels, so
 * pure yellow and pure blue both report 0.5 although one is nearly white and the
 * other nearly black. L* is perceptual and is what "how light is this" means
 * when the answer is being compared between two renders.
 */
export function lightness(rgb: Rgb): number {
  const linear = (value: number) => {
    const v = clampByte(value) / 255;
    return v <= 0.040_45 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  const y = 0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b);
  return y <= 216 / 24_389 ? (24_389 / 27) * y : 116 * Math.cbrt(y) - 16;
}

/*
  Hue names, by upper bound in degrees.

  Twelve rather than the six a colour wheel has: "blue" covering 180-260 degrees
  would call cyan and violet the same thing, which is precisely the distinction
  this readout exists to make. The names are the ones people reach for out loud,
  not the ones a standard prescribes.
*/
const HUE_NAMES: readonly (readonly [number, string])[] = [
  [14, "red"],
  [40, "orange"],
  [65, "yellow"],
  [95, "yellow-green"],
  [160, "green"],
  [190, "cyan"],
  [215, "sky blue"],
  [250, "blue"],
  [280, "violet"],
  [310, "purple"],
  [345, "magenta"],
  [360, "red"],
];

export function hueName(degrees: number): string {
  const h = ((degrees % 360) + 360) % 360;
  for (const [limit, name] of HUE_NAMES) {
    if (h < limit) return name;
  }
  return "red";
}

/**
 * Where a colour sits on the grey scale, ignoring whatever hue it carries.
 *
 * Keyed on **L\***, the same number the readout prints beside the word — not on
 * HSL's `l`. The two disagree by enough to read as a contradiction: `#A7A7A7` is
 * `l` 0.65 and L* 68.5, so a chip could say "grey" on one line and a lightness
 * of 68 on the next. The word is a gloss on the number; it has to be the same
 * number.
 */
function neutralName(lStar: number): string {
  if (lStar >= 0.95) return "white";
  if (lStar >= 0.85) return "near-white";
  if (lStar >= 0.66) return "light grey";
  if (lStar >= 0.45) return "grey";
  if (lStar >= 0.18) return "dark grey";
  if (lStar >= 0.04) return "near-black";
  return "black";
}

/**
 * How far from neutral a colour is, as a share of the 0-255 range.
 *
 * **Not HSL saturation**, and that is the point. `s` divides by how much room a
 * colour of that lightness has to be saturated, so the same three-code-value
 * lean reads as 1% on a mid grey and 20% on a near-white — which makes any
 * threshold expressed in `s` mean a different cast at every lightness. Measured
 * on this page: a 5-value blue lean on a light grey (`#BABCBF`) and a 20-value
 * one on a dark grey (`#56606A`) came out at 4% and 10%, so one cutoff could not
 * catch both without also naming single-value noise at the top of the range.
 *
 * The raw channel span has no such tilt: it is the distance from neutral in the
 * units the pixel is actually stored in, which is what "how strong is this cast"
 * means here.
 */
export function chroma(rgb: Rgb): number {
  const r = clampByte(rgb.r);
  const g = clampByte(rgb.g);
  const b = clampByte(rgb.b);
  return (Math.max(r, g, b) - Math.min(r, g, b)) / 255;
}

/**
 * Below this the hue is noise and is not reported at all.
 *
 * Four code values: a single pixel of a film scan carries two or three of grain
 * on its own, and in `fullsize` the readout *is* a single pixel. Naming a
 * direction read off grain would be the one thing worse than naming none.
 */
const NEUTRAL_CHROMA = 4 / 255;
/** Up to this a colour is a grey that leans; past it, it is a colour. */
const TINTED_CHROMA = 23 / 255;

/**
 * A short phrase for a colour — "light blue", "grey, slightly green", "black".
 *
 * Deliberately coarse. It is a label to say out loud beside the HEX, not a
 * measurement: the numbers are the measurement, and a phrase that tried to be
 * precise would be read as one.
 *
 * **The middle band is the reason this exists.** A faint cast — a grey a few
 * code values into green, a white a few into yellow — is precisely what an eye
 * cannot name, and it is what this readout was asked for. Calling it "grey"
 * throws away the finding; calling it "muted green" overstates a colour anyone
 * would read as grey. So a near-neutral is named on the grey scale *and* told
 * which way it leans.
 *
 * Below that band the hue is not reported at all: an angle derived from a span
 * of two or three code values is noise, and a direction invented from noise on a
 * page about colour casts is worse than saying nothing.
 */
export function describeColor(rgb: Rgb): string {
  const { h, s } = rgbToHsl(rgb);
  // L*, for the same reason `neutralName` takes it: the chip prints this number
  // right under the word, and a "light" beside an L* of 30 reads as a bug.
  const lStar = lightness(rgb) / 100;
  const cast = chroma(rgb);
  if (cast < NEUTRAL_CHROMA) return neutralName(lStar);
  if (cast < TINTED_CHROMA) return `${neutralName(lStar)}, slightly ${hueName(h)}`;
  const words: string[] = [];
  if (lStar >= 0.85) words.push("pale");
  else if (lStar >= 0.66) words.push("light");
  else if (lStar < 0.2) words.push("very dark");
  else if (lStar < 0.4) words.push("dark");
  // Only the low end is called out. "Vivid" on top of a hue name reads as
  // emphasis rather than as information, while "muted" changes which colour you
  // think is being named. Saturation is the right measure *here*, where the
  // question is how colourful a colour is rather than how far a grey leans.
  if (s < 0.3) words.push("muted");
  words.push(hueName(h));
  return words.join(" ");
}
