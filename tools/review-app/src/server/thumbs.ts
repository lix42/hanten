/**
 * Thumbnails for the preview strip.
 *
 * A review set names one file per rendition, and the strip shows that same file
 * in a 104x70 box. On sets of real scans that file is the scan — 5184x3600 and
 * ~7 MB — so a frame coming into view asks the browser to decode six 18.7 MP
 * photographs to paint six thumbnails. Measured on a 43-frame set: one `j` press
 * cost 832-888 ms, of which ~1 ms was script and the rest was a single
 * RasterTask full of `Decode Image`. Hiding the strip took the same press to
 * 16-32 ms.
 *
 * **Neither browser-side fix works, and both were measured before this existed.**
 * `decoding="async"` leaves the decode on the raster path (872 ms), and
 * pre-decoding the neighbours the overscan has already fetched does nothing
 * (872 ms) because an 18.7 MP image is ~75 MB decoded and Chrome's cache evicts
 * it long before you arrive. The only fix is to stop handing the browser a
 * 7 MB file for a 104 px box, which is what this does: libvips shrinks on load,
 * so the whole image is never decoded at full size at all.
 *
 * The result is cached on disk rather than in memory because the dev server
 * restarts often and the work is identical each time. The key carries the
 * source's mtime **and size**, for the same reason `Asset` does: a
 * timestamp-preserving rewrite must not go on serving the previous render.
 */

import { createHash, randomBytes } from "node:crypto";
import { chmod, mkdir, readFile, rename, stat, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

/**
 * What a thumbnail is keyed on: the file, as the caller has just seen it.
 *
 * Structurally an `Asset`, but taken as its own shape on purpose — the route
 * stats the file per request and passes *those* numbers, so the thumbnail and
 * the full-size response describe the same bytes. Keying on the set's load-time
 * stat instead let a re-render be served whole while the strip still answered
 * from the entry cached under the previous key.
 */
export interface ThumbnailSource {
  readonly path: string;
  readonly mtimeMs: number;
  readonly size: number;
}

/**
 * Width the strip's thumbnails are generated at: the 104px box at 2x.
 *
 * Stated once, on the server, and baked into the URL — the page never asks for
 * a size, so there is no negotiation to get wrong and no second definition to
 * drift. A retina panel is the case that matters; anything denser is a strip of
 * thumbnails, not the picture under review.
 */
export const PREVIEW_WIDTH = 208;

/**
 * Ceiling on a requested width.
 *
 * Only this server generates these URLs, so this is not a security boundary —
 * it is a cap on how much work one request can ask for, which keeps a
 * hand-typed `?w=99999` from turning into a full-size re-encode.
 */
export const MAX_THUMBNAIL_WIDTH = 2048;

/**
 * Identity of the sharp chain below. **Bump it whenever `generate` changes** in
 * a way that shows: colour handling, orientation, background, quality.
 *
 * It is folded into the cache key *and* into the preview URL
 * (`assets.thumbnailUrl`), and it has to be both. The disk cache is keyed on the
 * *source*, so a changed chain would not invalidate it; and the response is
 * `immutable` for a year, so with the source unchanged the URL would be
 * byte-identical and an already-loaded page would never ask again. The chain
 * that dropped the ICC profile was observed still being served from a browser
 * cache after the fix, which is why one of the two is not enough.
 */
export const PIPELINE_VERSION = 1;

/**
 * The width a request asked for, or `undefined` to serve the original file.
 *
 * Anything that is not a plain positive integer inside the cap reads as "no
 * thumbnail" rather than an error: the fallback is the file itself, which is
 * always correct, just slower.
 */
export function parseThumbnailWidth(raw: string | null): number | undefined {
  if (raw === null) return undefined;
  if (!/^\d+$/.test(raw)) return undefined;
  const width = Number.parseInt(raw, 10);
  if (width < 1 || width > MAX_THUMBNAIL_WIDTH) return undefined;
  return width;
}

/** The parts of a sharp `metadata()` the rule below reads. */
export interface ThumbnailProbe {
  readonly pages?: number | undefined;
  readonly width?: number | undefined;
}

/**
 * Why this file should be served as it is, or `undefined` to shrink it.
 *
 * Kept a pure function of the probe because it is the half worth testing and
 * the half that cannot be exercised through the real chain: sharp declines to
 * *write* an animated file from raw frames, so an animated fixture cannot be
 * built in memory to drive it end to end.
 *
 * **Animation is a property of the file, not of its extension.** `.gif` is off
 * the thumbnailable list for that reason, but WebP, AVIF and PNG all animate
 * too, and a strip quietly showing frame one beside a moving picture is exactly
 * the wrongness the extension rule was reaching for.
 *
 * **Already inside the box is the other.** `withoutEnlargement` declines to
 * resize such a file and then the chain re-encodes it anyway — a lossy
 * generation, sometimes *larger* than the source, for a picture the strip could
 * have shown untouched. A set that states its own small `preview` is this case.
 */
export function unsuitableReason(probe: ThumbnailProbe, width: number): string | undefined {
  if ((probe.pages ?? 1) > 1) return "animated";
  if (probe.width !== undefined && probe.width <= width) {
    return "already within the preview width";
  }
  return undefined;
}

/**
 * Whether shrinking this file is worth doing at all.
 *
 * SVG and GIF are left alone: an SVG is vector — the browser draws it at any
 * size for nothing, and rasterizing would be both slower and a worse picture —
 * and a GIF may be animated, which a thumbnail would silently freeze. Everything
 * else on the list is a raster photograph, which is exactly the expensive case.
 * An extension nothing recognises also serves as-is, so an unknown format is
 * never mangled into a JPEG.
 *
 * Every entry here must also have a content type in `assets.ts`: generation can
 * decline (a format this libvips build lacks, a corrupt file), and the fallback
 * then serves the original, which is a blank frame if the browser is told
 * `application/octet-stream`.
 */
export const THUMBNAILABLE: ReadonlySet<string> = new Set([
  ".avif",
  ".heic",
  ".heif",
  ".jpeg",
  ".jpg",
  ".png",
  ".tif",
  ".tiff",
  ".webp",
]);

export function isThumbnailable(path: string): boolean {
  const dot = path.lastIndexOf(".");
  return dot >= 0 && THUMBNAILABLE.has(path.slice(dot).toLowerCase());
}

/** Cache file name for one source at one width. Collisions are not a concern. */
export function thumbnailKey(source: ThumbnailSource, width: number): string {
  const hash = createHash("sha1")
    .update(
      `${source.path}\n${String(source.mtimeMs)}\n${String(source.size)}\n${String(width)}\n` +
        `v${String(PIPELINE_VERSION)}`,
    )
    .digest("hex")
    .slice(0, 20);
  return `${hash}.jpg`;
}

/**
 * Where generated thumbnails live: throwaway, rebuildable, outside the set.
 *
 * Named per uid because `tmpdir()` is per-user on macOS (`/var/folders/…/T`)
 * but `/tmp` on Linux, shared by everyone on the box. A second user's
 * `mkdir -p` against the first user's directory succeeds silently and every
 * write after it fails `EACCES` — a warning per request and no caching, for as
 * long as that directory exists. The uid separates users but does not *protect*
 * one from another; `ensureDir` is what makes the directory owner-only.
 * `process.getuid` rather than `os.userInfo()`:
 * it answers from the process, so it cannot throw for a uid with no passwd
 * entry, which would take the whole module — and with it the server — down.
 */
export const THUMBNAIL_DIR = join(
  tmpdir(),
  process.getuid === undefined
    ? "nc-review-app-thumbs"
    : `nc-review-app-thumbs-${String(process.getuid())}`,
);

/**
 * One generation at a time per cache key.
 *
 * A frame coming into view requests its six thumbnails at once, and a set often
 * names one file as several configs' rendition, so without this the same image
 * is decoded several times over — the very cost this module exists to remove.
 */
const inFlight = new Map<string, Promise<ThumbnailResult>>();

/**
 * Keys generation has already declined, so it is asked once.
 *
 * A rendition sharp cannot read fails the same way every time, and its frame
 * scrolls into view repeatedly — without this each pass re-reads the file, goes
 * back into libvips and prints another warning, which is exactly the per-view
 * cost this module exists to remove. In memory only: a restart is a fair moment
 * to try again.
 */
const declined = new Set<string>();

/**
 * Keys that should never be shrunk, and why — animated, or already small.
 *
 * Separate from `declined` because the answer is permanent for as long as the
 * file is: it is remembered so the probe runs once, and the route may cache it
 * for as long as it caches the file itself.
 */
const unsuitable = new Map<string, string>();

/**
 * What to serve for one preview request.
 *
 * Three outcomes, not two, because "serve the original" arrives for two
 * unrelated reasons and they must not be cached alike: a *failed attempt* (fd
 * exhaustion, a read-only temp dir, a format this libvips build lacks) may
 * succeed on the next request, while a file that should not be shrunk at all —
 * animated, or already smaller than the box — will never change its mind. One
 * enum rather than a boolean pair, so an impossible combination cannot be
 * spelled.
 */
export type ThumbnailResult =
  | { readonly kind: "thumbnail"; readonly body: Buffer }
  /** Serve the file itself, and keep that answer as long as the file lives. */
  | { readonly kind: "unsuitable"; readonly why: string }
  /** Serve the file itself, but ask again soon — this was one bad attempt. */
  | { readonly kind: "declined" };

/**
 * A thumbnail of `source` at `width`, or a reason to serve the original.
 *
 * Failure is never fatal: a file sharp cannot read, a missing install, a
 * read-only temp dir all fall back to the full-size file, which is what the
 * server did before thumbnails existed. A slow strip beats an empty one.
 */
export async function thumbnail(source: ThumbnailSource, width: number): Promise<ThumbnailResult> {
  const key = thumbnailKey(source, width);
  // The disk first, and only then the decline: a transient read failure — fd
  // exhaustion under six concurrent previews plus libvips' own handles — reads
  // as a cache miss, and sharp then fails the same way, so consulting the
  // decline first would retire a perfectly good entry on disk for the life of
  // the process. The read it saves is negligible; a declined key rarely has one.
  const cached = await readCached(key);
  if (cached) return { kind: "thumbnail", body: cached };
  const known = unsuitable.get(key);
  if (known !== undefined) return { kind: "unsuitable", why: known };
  if (declined.has(key)) return { kind: "declined" };

  const running = inFlight.get(key);
  if (running) return running;

  const work = generate(source, width)
    .then(async (result) => {
      if (result.kind === "thumbnail") await writeCached(key, result.body);
      else if (result.kind === "unsuitable") unsuitable.set(key, result.why);
      else declined.add(key);
      return result;
    })
    .finally(() => inFlight.delete(key));
  inFlight.set(key, work);
  return work;
}

async function generate(source: ThumbnailSource, width: number): Promise<ThumbnailResult> {
  try {
    // Imported here, not at module scope: this is the only code in the app that
    // needs a native module, and keeping the import inside the call means a
    // platform with no prebuilt binary degrades to serving originals instead of
    // failing to start.
    const { default: sharp } = await import("sharp");
    const image = sharp(source.path, { failOn: "none" });

    // One header read, reused by the pipeline below, answering the questions an
    // extension cannot.
    const why = unsuitableReason(await image.metadata(), width);
    if (why !== undefined) return { kind: "unsuitable", why };

    const body = await image
      // Before the resize, or a portrait scan tagged `Orientation` 6 is shrunk
      // on its stored axes and the tag is dropped, so the strip shows it
      // landscape while the stage shows it upright.
      .autoOrient()
      // Alpha has nowhere to go in a JPEG and libvips would composite it over
      // black, so a transparent rendition reads solid black in the strip.
      .flatten({ background: "#ffffff" })
      .resize({ width, withoutEnlargement: true })
      .jpeg({ quality: 80 })
      // sharp copies no input metadata unless asked. An untagged JPEG is read
      // as sRGB, so a Display P3 rendition would show visibly more saturated in
      // the strip than on the stage beside it — in a tool for judging colour by
      // eye. Preserved rather than converted, so the two match.
      .keepIccProfile()
      .toBuffer();
    return { kind: "thumbnail", body };
  } catch (cause) {
    console.warn(`could not thumbnail ${source.path}: ${describe(cause)}`);
    return { kind: "declined" };
  }
}

async function readCached(key: string): Promise<Buffer | undefined> {
  try {
    const body = await readFile(join(THUMBNAIL_DIR, key));
    // An empty `Buffer` is truthy, and the key only changes when the source
    // does — so a truncated entry, served as a 200 marked `immutable`, is a
    // broken image for that rendition on every later run. Treat it as a miss.
    return body.byteLength > 0 ? body : undefined;
  } catch {
    return undefined; // not generated yet, or the temp dir was cleaned
  }
}

/**
 * Create the cache directory owner-only, once per process.
 *
 * **These are derived from the user's own photographs, in a directory other
 * people can read.** On Linux `tmpdir()` is the shared `/tmp`, and `mkdir` under
 * the usual `0022` umask would leave the directory `0755` and its entries
 * `0644`: the uid in the name prevents a collision, but it is a name, not a
 * permission. A directory left over from an earlier run of this app is widened
 * back — only when we own it, since chmod on someone else's fails and the write
 * after it would have failed anyway.
 */
async function ensureDir(): Promise<void> {
  await mkdir(THUMBNAIL_DIR, { recursive: true, mode: 0o700 });
  if (tightened) return;
  tightened = true;
  const stats = await stat(THUMBNAIL_DIR);
  const ours = process.getuid === undefined || stats.uid === process.getuid();
  if (ours && (stats.mode & 0o077) !== 0) await chmod(THUMBNAIL_DIR, 0o700);
}

let tightened = false;

async function writeCached(key: string, body: Buffer): Promise<void> {
  const final = join(THUMBNAIL_DIR, key);
  const staging = `${final}.${randomBytes(8).toString("hex")}.tmp`;
  try {
    await ensureDir();
    // Written aside and renamed, never in place: `writeFile` truncates, so an
    // in-place write leaves the entry 0 bytes for its whole duration and a
    // reader in that window — or a server killed inside it — gets an empty
    // file. A rename within one directory is atomic.
    await writeFile(staging, body, { mode: 0o600 });
    await rename(staging, final);
  } catch (cause) {
    await unlink(staging).catch(() => undefined);
    // A cache that cannot be written still serves this request; every later one
    // regenerates and warns again. Deliberately not memoized the way a declined
    // *generation* is: a full or read-only temp dir is a condition that can
    // clear, and a cache that silently stopped trying would be worse than a
    // repeated line saying why every thumbnail is slow.
    console.warn(`could not cache a thumbnail in ${THUMBNAIL_DIR}: ${describe(cause)}`);
  }
}

function describe(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
