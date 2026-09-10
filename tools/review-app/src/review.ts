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

export type ZoomMode = "fit" | "fullsize";

export interface ReviewConfig {
  readonly id: string;
  readonly label: string;
  readonly note?: string;
}

export interface Rendition {
  /** URL the page loads the full image from, produced by the resolver. */
  readonly src: string;
  /** URL of the thumbnail; falls back to `src` when unstated. */
  readonly preview: string;
  readonly width?: number;
  readonly height?: number;
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
 * Turns a path written in a review file into the URL the page loads it from.
 *
 * Injected rather than fixed because the two sides resolve differently: the
 * server maps a path to a file on disk and hands back a `/img/` URL keyed to
 * it, while a test only needs something deterministic to assert on. `at` is the
 * document path of the offending field, for error messages.
 */
export type ResolveRendition = (path: string, at: string) => string;

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

function parseRendition(raw: unknown, resolve: ResolveRendition, at: string): Rendition {
  // Shorthand: a bare string is the src, which is all a generator usually has.
  if (typeof raw === "string") {
    const src = resolve(asString(raw, at), at);
    return { src, preview: src };
  }
  const record = asRecord(raw, at);
  const src = resolve(asString(record["src"], `${at}.src`), `${at}.src`);
  const previewRaw = optionalString(record["preview"], `${at}.preview`);
  return {
    src,
    preview: previewRaw ? resolve(previewRaw, `${at}.preview`) : src,
    ...dimensions(record, at),
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
export function parseReview(raw: unknown, resolve: ResolveRendition): Review {
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

  const images = asArray(doc["images"], "images").map((raw, index) => {
    const at = `images[${index}]`;
    const record = asRecord(raw, at);
    const id = asString(record["id"], `${at}.id`);
    const renditionsRaw = asRecord(record["renditions"], `${at}.renditions`);
    // `fromEntries` defines own properties, so a config id spelled `__proto__`
    // lands as data rather than silently setting the prototype.
    const renditions = Object.fromEntries(
      Object.entries(renditionsRaw).map(([configId, value]) => {
        if (!seen.has(configId)) {
          fail(
            `${at}.renditions names ${JSON.stringify(configId)}, which is not one of ` +
              `the declared configs (${configs.map((c) => c.id).join(", ")})`,
          );
        }
        return [configId, parseRendition(value, resolve, `${at}.renditions.${configId}`)] as const;
      }),
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
