/**
 * The subset of an `nctool metrics` record the charts read, and how it is turned
 * into the model they render.
 *
 * The file is JSON with `snake_case` keys, matching every other JSON contract in
 * this repo, because the producer is an nc-adjacent Python tool rather than
 * JavaScript. Parsing is the one place that spelling is known; everything
 * downstream uses the camelCase model below — the same division `review.ts` makes.
 *
 * **Nothing here is derived that the record states.** Band names, band edges, the
 * bin count and the two reference bins all come out of the record, because the
 * band cut has already changed once: `schema_version` 2 replaced five stops-even
 * bands with seven on equal L* steps. A chart holding its own copy of the edges
 * would have kept drawing, silently wrong.
 *
 * Only the charted subset is validated — a record carries far more, and refusing
 * a field this module has no opinion about would make the charts brittle against
 * a schema that is still moving. A few parsed fields are not yet *drawn*
 * (`domain`, `lstarEdges`, `nonFinite`, `nonPositive`, `cast.pixels`); they are
 * validated because a consumer that reaches for them should fail loudly rather
 * than read `undefined`.
 */

/** One channel of the histogram: bin counts, plus what fell outside them. */
export interface HistogramSeries {
  readonly counts: readonly number[];
  /** Samples brighter than the top of the range — not binned, counted here. */
  readonly aboveRange: number;
  readonly nonFinite: number;
  readonly nonPositive: number;
}

export interface Histogram {
  readonly bins: number;
  /** `cielab_lstar` today. Parsed so a chart *can* refuse a domain it cannot
   * draw — none does yet; the histogram assumes the L* domain. */
  readonly domain: string;
  readonly lstarRange: readonly [number, number];
  /** Reference lines the record names, so a chart never re-derives the L* curve. */
  readonly midGreyBin: number;
  readonly diffuseWhiteBin: number;
  readonly pixels: number;
  readonly series: Readonly<Record<string, HistogramSeries>>;
}

export interface Bands {
  /** Tone order, dark to light. **The only source of order** — see `parseMetrics`. */
  readonly names: readonly string[];
  readonly lstarEdges: readonly number[];
  readonly sparseBelowFraction: number;
}

/** One band's mean colour cast. `sparse` marks a band too small to be a measurement. */
export interface CastBand {
  readonly band: string;
  readonly meanA: number;
  readonly meanB: number;
  readonly fraction: number;
  readonly pixels: number;
  readonly sparse: boolean;
}

export interface Metrics {
  readonly schemaVersion: number;
  readonly bands: Bands;
  readonly histogram: Histogram;
  /** In tone order, and containing only the bands the record actually carries. */
  readonly cast: readonly CastBand[];
}

/** The lowest `schema_version` carrying `tone.histogram` and the L* band cut. */
export const MIN_SCHEMA_VERSION = 2;

class MetricsError extends Error {}

function fail(message: string): never {
  throw new MetricsError(message);
}

function describe(value: unknown): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return "an array";
  return typeof value;
}

function asRecord(value: unknown, at: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(`${at} must be an object, got ${describe(value)}`);
  }
  return value as Record<string, unknown>;
}

function asNumber(value: unknown, at: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    fail(`${at} must be a finite number, got ${describe(value)}`);
  }
  return value;
}

function asBoolean(value: unknown, at: string): boolean {
  if (typeof value !== "boolean") {
    fail(`${at} must be a boolean, got ${describe(value)}`);
  }
  return value;
}

function asString(value: unknown, at: string): string {
  if (typeof value !== "string" || value === "") {
    fail(`${at} must be a non-empty string, got ${describe(value)}`);
  }
  return value;
}

function asNumberArray(value: unknown, at: string): number[] {
  if (!Array.isArray(value)) fail(`${at} must be an array, got ${describe(value)}`);
  return value.map((v, i) => asNumber(v, `${at}[${i}]`));
}

function asStringArray(value: unknown, at: string): string[] {
  if (!Array.isArray(value)) fail(`${at} must be an array, got ${describe(value)}`);
  return value.map((v, i) => asString(v, `${at}[${i}]`));
}

function parseSeries(raw: unknown, bins: number, at: string): HistogramSeries {
  const record = asRecord(raw, at);
  const counts = asNumberArray(record["counts"], `${at}.counts`);
  if (counts.length !== bins) {
    fail(`${at}.counts has ${counts.length} bins, but the histogram declares ${bins}`);
  }
  return {
    counts,
    aboveRange: asNumber(record["above_range"], `${at}.above_range`),
    nonFinite: asNumber(record["non_finite"], `${at}.non_finite`),
    nonPositive: asNumber(record["non_positive"], `${at}.non_positive`),
  };
}

/**
 * Parse the charted subset of a metrics record.
 *
 * Throws with a message naming the offending path rather than returning a partial
 * model — half a chart is worse than a refusal, because the missing half is
 * invisible.
 */
export function parseMetrics(raw: unknown): Metrics {
  const doc = asRecord(raw, "the metrics record");

  const schemaVersion = asNumber(doc["schema_version"], "schema_version");
  if (schemaVersion < MIN_SCHEMA_VERSION) {
    fail(
      `schema_version must be at least ${MIN_SCHEMA_VERSION} (which added ` +
        `tone.histogram and the L* band cut), got ${schemaVersion}`,
    );
  }

  const bandsRaw = asRecord(doc["bands"], "bands");
  const bands: Bands = {
    names: asStringArray(bandsRaw["names"], "bands.names"),
    lstarEdges: asNumberArray(bandsRaw["lstar_edges"], "bands.lstar_edges"),
    sparseBelowFraction: asNumber(bandsRaw["sparse_below_fraction"], "bands.sparse_below_fraction"),
  };
  if (bands.names.length === 0) fail("bands.names must list at least one band");

  const tone = asRecord(doc["tone"], "tone");
  const histRaw = asRecord(tone["histogram"], "tone.histogram");
  const bins = asNumber(histRaw["bins"], "tone.histogram.bins");
  const range = asNumberArray(histRaw["lstar_range"], "tone.histogram.lstar_range");
  if (range.length !== 2) {
    fail(`tone.histogram.lstar_range must hold two numbers, got ${range.length}`);
  }
  const seriesRaw = asRecord(histRaw["series"], "tone.histogram.series");
  const histogram: Histogram = {
    bins,
    domain: asString(histRaw["domain"], "tone.histogram.domain"),
    lstarRange: [range[0] ?? 0, range[1] ?? 0],
    midGreyBin: asNumber(histRaw["mid_grey_bin"], "tone.histogram.mid_grey_bin"),
    diffuseWhiteBin: asNumber(histRaw["diffuse_white_bin"], "tone.histogram.diffuse_white_bin"),
    pixels: asNumber(histRaw["pixels"], "tone.histogram.pixels"),
    series: Object.assign(
      Object.create(null) as Record<string, HistogramSeries>,
      Object.fromEntries(
        Object.entries(seriesRaw).map(([name, value]) => [
          name,
          parseSeries(value, bins, `tone.histogram.series.${name}`),
        ]),
      ),
    ),
  };

  // **Tone order comes from `bands.names`, never from the object's own keys.**
  // `cast_by_tone_band` is a JSON object, so iterating it yields *alphabetical*
  // order — `deep_shadow, high_mid, highlight, low_mid, mid, shadow` — which is
  // tone order scrambled, and which plots a smooth, plausible, wrong curve. A
  // band the record omits is ordinary (no pixels landed there) and is skipped.
  const castRaw = asRecord(
    asRecord(doc["color"], "color")["cast_by_tone_band"],
    "color.cast_by_tone_band",
  );
  for (const name of Object.keys(castRaw)) {
    if (!bands.names.includes(name)) {
      fail(
        `color.cast_by_tone_band names ${JSON.stringify(name)}, which is not one of ` +
          `bands.names (${bands.names.join(", ")})`,
      );
    }
  }
  const cast: CastBand[] = bands.names.flatMap((band) => {
    const entry = castRaw[band];
    if (entry === undefined) return [];
    const at = `color.cast_by_tone_band.${band}`;
    const record = asRecord(entry, at);
    return [
      {
        band,
        meanA: asNumber(record["mean_a"], `${at}.mean_a`),
        meanB: asNumber(record["mean_b"], `${at}.mean_b`),
        fraction: asNumber(record["fraction"], `${at}.fraction`),
        pixels: asNumber(record["pixels"], `${at}.pixels`),
        // Checked like every neighbouring field rather than coerced: a missing
        // or mistyped flag would otherwise draw a one-pixel band at full weight,
        // which is precisely what `sparse` exists to prevent.
        sparse: asBoolean(record["sparse"], `${at}.sparse`),
      },
    ];
  });

  return { schemaVersion, bands, histogram, cast };
}
