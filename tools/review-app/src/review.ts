/**
 * The review-set schema: what a `review.json` contains, and how it is turned
 * into the model the UI renders.
 *
 * The file is JSON with `snake_case` keys, matching every other JSON contract in
 * this repo (nc's reports and recipes), because the producers are nc-adjacent
 * scripts rather than JavaScript. Parsing is the one place that spelling is
 * known; everything downstream uses the camelCase model below.
 *
 * **Unknown config ids are rejected, missing renditions are not.** A rendition
 * keyed by a config that does not exist is a typo the author wants to hear about
 * — the same reason every recipe struct in this repo uses `deny_unknown_fields`.
 * A config with no rendition for one image is ordinary (a config that failed to
 * render, a frame added later), so it renders as a visible gap instead.
 */

import type { Metrics } from "./charts/metrics";

export type ZoomMode = "fit" | "fullsize";

export interface ReviewConfig {
  readonly id: string;
  readonly label: string;
  readonly note?: string;
}

export interface Rendition {
  /** URL the page loads the full image from, produced by the resolver. */
  readonly src: string;
  /**
   * URL the preview strip loads, for the same image `src` names.
   *
   * A separate URL rather than the same one because the server answers the
   * preview role with a size (`&w=`); a set that states no `preview` path
   * still gets one, resolved from `src`.
   */
  readonly preview: string;
  readonly width?: number;
  readonly height?: number;
  /** The measurement of *these* pixels, when the set names a record for them. */
  readonly metrics?: Metrics;
  /**
   * Why a named record could not be used, shown where the charts would be.
   *
   * A broken measurement must not cost the comparison: the picture is the thing
   * this app exists to show, and refusing the whole set over one unreadable
   * record would take every other config down with it. Loud, but local.
   */
  readonly metricsError?: string;
}

export interface ReviewImage {
  readonly id: string;
  readonly label: string;
  readonly note?: string;
  /**
   * Keyed by config id. A config absent here has no rendition for this image.
   *
   * A plain object rather than a `Map` so the model is exactly what crosses the
   * wire — Start's serializable check refuses a `ReadonlyMap`, and the JSON this
   * came from is an object anyway.
   */
  readonly renditions: Readonly<Record<string, Rendition | undefined>>;
}

export interface Review {
  readonly title?: string;
  readonly description?: string;
  readonly configs: readonly ReviewConfig[];
  readonly images: readonly ReviewImage[];
}

/** The only schema version this build understands. */
export const SCHEMA_VERSION = 1;

class ReviewError extends Error {}

function fail(message: string): never {
  throw new ReviewError(message);
}

function asRecord(value: unknown, at: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(`${at} must be an object, got ${describe(value)}`);
  }
  return value as Record<string, unknown>;
}

function asArray(value: unknown, at: string): unknown[] {
  if (!Array.isArray(value)) fail(`${at} must be an array, got ${describe(value)}`);
  return value;
}

function asString(value: unknown, at: string): string {
  if (typeof value !== "string" || value === "") {
    fail(`${at} must be a non-empty string, got ${describe(value)}`);
  }
  return value;
}

function optionalString(value: unknown, at: string): string | undefined {
  if (value === undefined || value === null) return undefined;
  return asString(value, at);
}

function optionalPositiveInt(value: unknown, at: string): number | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value !== "number" || !Number.isInteger(value) || value <= 0) {
    fail(`${at} must be a positive integer, got ${describe(value)}`);
  }
  return value;
}

function describe(value: unknown): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return "an array";
  return typeof value;
}

/**
 * What a resolved URL is going to be used for.
 *
 * The strip shows the same file as the stage, in a 104px box, so the two roles
 * want different bytes of the same path — the server answers `preview` with a
 * downscaled URL. Passed as a role rather than sniffed from `at`, because which
 * URL a caller gets is a decision, not a string pattern.
 */
export type RenditionRole = "src" | "preview";

/**
 * Turns a path written in a review file into the URL the page loads it from.
 *
 * Injected rather than fixed because the two sides resolve differently: the
 * server maps a path to a file on disk and hands back a `/img/` URL keyed to
 * it, while a test only needs something deterministic to assert on. `at` is the
 * document path of the offending field, for error messages.
 */
export type ResolveRendition = (path: string, at: string, role: RenditionRole) => string;

/**
 * Turns a metrics path written in a review file into that rendition's record.
 *
 * Injected for the same reason `ResolveRendition` is, and it *reads* as well as
 * resolves: the server reaches the filesystem, a test hands back a stub. Optional
 * so a caller with no filesystem — every test that is not about measurements —
 * gets a document without them.
 *
 * It returns a failure rather than throwing, because a record that cannot be
 * read is a missing chart, not a refused review set.
 */
export type LoadMetrics = (
  path: string,
  at: string,
) => { metrics: Metrics } | { metricsError: string };

/**
 * `width`/`height` are optional, but only *together*.
 *
 * They exist so the page can reserve the right box before the image arrives, and
 * half of a size reserves nothing. A rendition stating one and not the other is a
 * half-finished edit, so it is refused rather than silently ignored — which is also
 * what keeps the load-time mismatch check from skipping such a rendition.
 */
function dimensions(
  record: Record<string, unknown>,
  at: string,
): { width?: number; height?: number } {
  const width = optionalPositiveInt(record["width"], `${at}.width`);
  const height = optionalPositiveInt(record["height"], `${at}.height`);
  if ((width === undefined) !== (height === undefined)) {
    fail(
      `${at} states ${width === undefined ? "height" : "width"} without the other; ` +
        `give both or neither — half a size cannot reserve space for the image`,
    );
  }
  return { width, height };
}

function parseRendition(
  raw: unknown,
  resolve: ResolveRendition,
  loadMetrics: LoadMetrics | undefined,
  at: string,
): Rendition {
  // Shorthand: a bare string is the src, which is all a generator usually has.
  if (typeof raw === "string") {
    const path = asString(raw, at);
    return { src: resolve(path, at, "src"), preview: resolve(path, at, "preview") };
  }
  const record = asRecord(raw, at);
  const srcPath = asString(record["src"], `${at}.src`);
  const src = resolve(srcPath, `${at}.src`, "src");
  const previewRaw = optionalString(record["preview"], `${at}.preview`);
  const metricsRaw = optionalString(record["metrics"], `${at}.metrics`);
  return {
    src,
    // **The preview role applies to a declared thumbnail too**, not only to the
    // fallback. A set that names one has no idea how big the strip's box is, and
    // a thumbnail already small enough is not resized — so one rule ("the strip
    // is served downscaled") beats two, and a set that points `preview` at
    // another full-size file gets the same protection as one that omits it.
    preview: resolve(previewRaw ?? srcPath, `${at}.preview`, "preview"),
    ...dimensions(record, at),
    ...(metricsRaw && loadMetrics ? loadMetrics(metricsRaw, `${at}.metrics`) : {}),
  };
}

/**
 * Parse and validate a review document.
 *
 * Every rendition path goes through `resolve`, so a review file and its images
 * travel together as one directory whatever the caller turns those paths into.
 *
 * Throws with a message naming the offending path (`images[2].renditions.none`)
 * rather than returning a partial model — a half-loaded comparison is worse than
 * a refusal, because the missing half is invisible.
 */
export function parseReview(
  raw: unknown,
  resolve: ResolveRendition,
  loadMetrics?: LoadMetrics,
): Review {
  const doc = asRecord(raw, "the review document");

  const version = doc["schema_version"];
  if (version !== SCHEMA_VERSION) {
    fail(
      `schema_version must be ${SCHEMA_VERSION}, got ${describe(version)} ${JSON.stringify(version) ?? ""}`.trim(),
    );
  }

  const configs = asArray(doc["configs"], "configs").map((raw, index) => {
    const at = `configs[${index}]`;
    const record = asRecord(raw, at);
    return {
      id: asString(record["id"], `${at}.id`),
      label: asString(record["label"], `${at}.label`),
      note: optionalString(record["note"], `${at}.note`),
    } satisfies ReviewConfig;
  });
  if (configs.length === 0) fail("configs must list at least one configuration");

  const seen = new Set<string>();
  for (const config of configs) {
    if (seen.has(config.id)) {
      fail(`configs contains two entries with id ${JSON.stringify(config.id)}`);
    }
    seen.add(config.id);
  }

  // Image ids are unique for the same reason config ids are, plus one the
  // charts add: a chart's SVG gradients are addressed by an id built from the
  // image and the config, and two `<linearGradient id="E1-a">` in one document
  // both resolve to the *first*. The second frame's cast curves would then be
  // painted with the first frame's value-to-colour mapping — and on that chart
  // colour is the encoding.
  const imageIds = new Set<string>();
  const images = asArray(doc["images"], "images").map((raw, index) => {
    const at = `images[${index}]`;
    const record = asRecord(raw, at);
    const id = asString(record["id"], `${at}.id`);
    if (imageIds.has(id)) fail(`images contains two entries with id ${JSON.stringify(id)}`);
    imageIds.add(id);
    const renditionsRaw = asRecord(record["renditions"], `${at}.renditions`);
    // Two hazards, one on each side of the lookup. `fromEntries` defines own
    // properties, so a config id spelled `__proto__` lands as data rather than
    // silently setting the prototype; and the result gets a **null prototype**,
    // so reading a config id that happens to name an `Object.prototype` member
    // (`toString`, `constructor`) yields `undefined` rather than an inherited
    // function — which would read as a present rendition and render a broken
    // image where the missing-rendition gap belongs. The `Map` this replaced
    // had neither hazard.
    const renditions: Record<string, Rendition> = Object.assign(
      Object.create(null) as Record<string, Rendition>,
      Object.fromEntries(
        Object.entries(renditionsRaw).map(([configId, value]) => {
          if (!seen.has(configId)) {
            fail(
              `${at}.renditions names ${JSON.stringify(configId)}, which is not one of ` +
                `the declared configs (${configs.map((c) => c.id).join(", ")})`,
            );
          }
          return [
            configId,
            parseRendition(value, resolve, loadMetrics, `${at}.renditions.${configId}`),
          ] as const;
        }),
      ),
    );
    return {
      id,
      label: optionalString(record["label"], `${at}.label`) ?? id,
      note: optionalString(record["note"], `${at}.note`),
      renditions,
    } satisfies ReviewImage;
  });

  return {
    title: optionalString(doc["title"], "title"),
    description: optionalString(doc["description"], "description"),
    configs,
    images,
  };
}
