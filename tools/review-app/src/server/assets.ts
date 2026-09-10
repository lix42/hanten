/**
 * The set of image files a review set is allowed to serve.
 *
 * Renditions are named by paths relative to `review.json`, and a set may
 * legitimately point outside its own directory (`../renders/E1.jpg`). Rather
 * than confine the server to a root — which would refuse those sets, and still
 * needs traversal checks — every rendition seen while parsing is registered
 * here and addressed by an opaque id afterwards. A path that is not in the map
 * cannot be requested, so `..` in a URL means nothing.
 *
 * The id is derived from the absolute path so it survives a reload; the mtime
 * rides along as `?v=`, so re-rendering a frame changes its URL and the browser
 * refetches exactly that one image.
 */

import { createHash } from "node:crypto";

/** Modification time in milliseconds, or `undefined` when the file is absent. */
export type StatMtime = (absolutePath: string) => number | undefined;

export interface Asset {
  readonly path: string;
  /** 0 when the file did not exist at load time; the request will 404. */
  readonly mtimeMs: number;
}

export interface AssetMap {
  /** Register a file and return the URL the page should load it from. */
  register(absolutePath: string): string;
  get(id: string): Asset | undefined;
  /** Every registered file, for the watcher to compare against. */
  entries(): readonly Asset[];
}

/** Ids are opaque; 16 hex chars is far past collision risk for one review set. */
export function assetId(absolutePath: string): string {
  return createHash("sha1").update(absolutePath).digest("hex").slice(0, 16);
}

export function assetUrl(id: string, mtimeMs: number): string {
  return `/img/${id}?v=${String(mtimeMs)}`;
}

export function createAssetMap(stat: StatMtime): AssetMap {
  const assets = new Map<string, Asset>();
  return {
    register(absolutePath) {
      const id = assetId(absolutePath);
      // Registering the same file twice is ordinary — a rendition reused as its
      // own preview, or one image shared by two configs. Stat it once.
      let asset = assets.get(id);
      if (!asset) {
        asset = { path: absolutePath, mtimeMs: stat(absolutePath) ?? 0 };
        assets.set(id, asset);
      }
      return assetUrl(id, asset.mtimeMs);
    },
    get: (id) => assets.get(id),
    entries: () => [...assets.values()],
  };
}

/**
 * Content type for an image path, by extension.
 *
 * Deliberately a closed list: anything else is served as a download rather than
 * guessed at, which keeps a stray `.json` or `.html` in a set directory from
 * being served as active content.
 */
const CONTENT_TYPES: Readonly<Record<string, string>> = {
  ".avif": "image/avif",
  ".gif": "image/gif",
  ".jpeg": "image/jpeg",
  ".jpg": "image/jpeg",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".tif": "image/tiff",
  ".tiff": "image/tiff",
  ".webp": "image/webp",
};

export function contentTypeFor(path: string): string {
  const dot = path.lastIndexOf(".");
  if (dot < 0) return "application/octet-stream";
  return CONTENT_TYPES[path.slice(dot).toLowerCase()] ?? "application/octet-stream";
}
