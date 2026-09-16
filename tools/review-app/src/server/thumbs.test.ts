import { mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vite-plus/test";
import { contentTypeFor } from "./assets";
import {
  MAX_THUMBNAIL_WIDTH,
  THUMBNAILABLE,
  THUMBNAIL_DIR,
  type ThumbnailSource,
  isThumbnailable,
  parseThumbnailWidth,
  unsuitableReason,
  thumbnail,
  thumbnailKey,
} from "./thumbs";

/**
 * sharp, if this platform has a prebuilt binary for it.
 *
 * Imported dynamically and tolerantly, the way `thumbs.ts` imports it: the point
 * of that import living inside the call is that a platform with no binary serves
 * originals instead of failing to start, and a static import here would take the
 * whole suite down at collection on exactly that platform.
 */
const sharp = await import("sharp").then(
  (module) => module.default,
  (cause: unknown) => {
    console.warn(`sharp is unavailable; skipping the thumbnail chain tests: ${String(cause)}`);
    return undefined;
  },
);

const ASSET: ThumbnailSource = {
  path: "/sets/tone/E1-shoulder.jpg",
  mtimeMs: 1700,
  size: 7_193_516,
};

describe("parseThumbnailWidth", () => {
  it("reads the width the strip asked for", () => {
    expect(parseThumbnailWidth("208")).toBe(208);
    expect(parseThumbnailWidth("1")).toBe(1);
    expect(parseThumbnailWidth(String(MAX_THUMBNAIL_WIDTH))).toBe(MAX_THUMBNAIL_WIDTH);
  });

  it("answers undefined — serve the file — for anything else", () => {
    // Not an error: the full-size file is always a correct answer, just slower,
    // so a malformed width degrades instead of failing the image.
    for (const raw of [null, "", "0", "-1", "12.5", "1e3", " 208", "208px", "abc"]) {
      expect(parseThumbnailWidth(raw)).toBeUndefined();
    }
  });

  it("caps how much work one request can ask for", () => {
    expect(parseThumbnailWidth(String(MAX_THUMBNAIL_WIDTH + 1))).toBeUndefined();
    expect(parseThumbnailWidth("99999")).toBeUndefined();
  });
});

describe("isThumbnailable", () => {
  it("shrinks raster photographs, whatever the case of the extension", () => {
    for (const path of [
      "/a/b.jpg",
      "/a/b.JPEG",
      "/a/b.png",
      "/a/b.tif",
      "/a/b.webp",
      "/a/b.avif",
    ]) {
      expect(isThumbnailable(path)).toBe(true);
    }
  });

  it("leaves vector, animated and unknown files alone", () => {
    // An SVG costs the browser nothing at any size and would only get worse as a
    // JPEG; a GIF may be animated; an unknown extension must not be mangled into
    // a format the set did not ask for.
    for (const path of ["/a/b.svg", "/a/b.gif", "/a/b.mp4", "/a/b", "/a/b.jpg.txt"]) {
      expect(isThumbnailable(path)).toBe(false);
    }
  });
});

describe("thumbnailKey", () => {
  it("names a JPEG, since that is what is generated", () => {
    expect(thumbnailKey(ASSET, 208)).toMatch(/^[0-9a-f]{20}\.jpg$/);
  });

  it("separates sizes, so one width never serves another", () => {
    expect(thumbnailKey(ASSET, 208)).not.toBe(thumbnailKey(ASSET, 416));
  });

  it("follows the source's mtime and size", () => {
    // Size as well as mtime, for the reason `Asset` carries both: a
    // timestamp-preserving copy (`cp -p`, `rsync -t`) leaves the mtime alone,
    // and a cache keyed on mtime only would go on serving the previous render.
    expect(thumbnailKey({ ...ASSET, mtimeMs: 1800 }, 208)).not.toBe(thumbnailKey(ASSET, 208));
    expect(thumbnailKey({ ...ASSET, size: 7_193_517 }, 208)).not.toBe(thumbnailKey(ASSET, 208));
  });

  it("gives the same source the same name every run, so a restart reuses it", () => {
    expect(thumbnailKey(ASSET, 208)).toBe(thumbnailKey({ ...ASSET }, 208));
    expect(thumbnailKey({ ...ASSET, path: "/sets/tone/E1-soft.jpg" }, 208)).not.toBe(
      thumbnailKey(ASSET, 208),
    );
  });
});

describe("unsuitableReason", () => {
  it("shrinks an ordinary still picture", () => {
    expect(unsuitableReason({ pages: 1, width: 5184 }, 208)).toBeUndefined();
    // No `pages` at all is the common case — a JPEG says nothing about frames.
    expect(unsuitableReason({ width: 5184 }, 208)).toBeUndefined();
  });

  it("serves an animation as it is, whatever its extension", () => {
    // `.gif` is off the thumbnailable list for this reason, but WebP, AVIF and
    // PNG animate too, and the strip must not freeze one beside a moving stage.
    expect(unsuitableReason({ pages: 2, width: 5184 }, 208)).toBe("animated");
  });

  it("serves a file already inside the box rather than re-encoding it", () => {
    // Lossy work, sometimes producing *more* bytes, for no change in size.
    expect(unsuitableReason({ width: 100 }, 208)).toBe("already within the preview width");
    // Exactly the box is already inside it: resizing would be a no-op too.
    expect(unsuitableReason({ width: 208 }, 208)).toBe("already within the preview width");
    expect(unsuitableReason({ width: 209 }, 208)).toBeUndefined();
  });

  it("shrinks when the width is unknown, rather than declining blind", () => {
    // An unreadable header is the chain's problem, not this rule's: let it try
    // and fail loudly rather than silently serving every such file whole.
    expect(unsuitableReason({}, 208)).toBeUndefined();
  });
});

describe("THUMBNAILABLE", () => {
  it("names only formats the fallback can state a content type for", () => {
    // Generation may decline — a format this libvips build lacks, a corrupt
    // file — and the route then serves the original. A browser told
    // `application/octet-stream` shows nothing, so a format that is
    // thumbnailable but has no content type is a blank frame.
    //
    // A stated type is all this pins, and all it can: whether a browser then
    // *renders* the format is the browser's business — `image/tiff` and
    // `image/heic` are honest types that Chrome still declines to draw.
    for (const extension of THUMBNAILABLE) {
      expect(contentTypeFor(`/a/b${extension}`)).not.toBe("application/octet-stream");
    }
  });
});

/**
 * The sharp chain itself, on images built in memory.
 *
 * Every assertion here fails against the chain as first written, which is the
 * point: each pins one property a thumbnail must share with the stage picture
 * beside it.
 */
(sharp ? describe : describe.skip)("thumbnail", () => {
  const lib = sharp!;
  let dir = "";
  const generated: string[] = [];

  beforeAll(async () => {
    dir = await mkdtemp(join(tmpdir(), "nc-review-app-thumbs-test-"));
  });

  afterAll(async () => {
    await rm(dir, { recursive: true, force: true });
    // Only this run's own entries: two servers on two sets share the directory.
    for (const key of generated) await rm(join(THUMBNAIL_DIR, key), { force: true });
  });

  /** Write `body` into the scratch directory and key it the way the route does. */
  async function source(name: string, body: Buffer): Promise<ThumbnailSource> {
    const path = join(dir, name);
    await writeFile(path, body);
    const stats = await stat(path);
    const at = { path, mtimeMs: stats.mtimeMs, size: stats.size };
    generated.push(thumbnailKey(at, 10));
    return at;
  }

  /** A 40x20 image, so a resize to width 10 lands on 10x5 unless it is rotated. */
  function canvas(channels: 3 | 4 = 3) {
    return lib({
      create: {
        width: 40,
        height: 20,
        channels,
        background: channels === 4 ? { r: 0, g: 0, b: 0, alpha: 0 } : "#336699",
      },
    });
  }

  /** Run `body` with `console.warn` captured, and answer what it said. */
  async function quietly(body: () => Promise<void>): Promise<unknown[]> {
    const said: unknown[] = [];
    const original = console.warn;
    console.warn = (...args: unknown[]) => said.push(args);
    try {
      await body();
    } finally {
      console.warn = original;
    }
    return said;
  }

  /** The thumbnail bytes, failing the test if the chain declined to make any. */
  async function bytesOf(at: ThumbnailSource, width: number): Promise<Buffer> {
    const result = await thumbnail(at, width);
    expect(result.kind).toBe("thumbnail");
    if (result.kind !== "thumbnail") throw new Error("no thumbnail");
    return result.body;
  }

  it("keeps the source's colour profile, so the strip matches the stage", async () => {
    // An untagged JPEG is read as sRGB: a Display P3 rendition served without
    // its profile shows visibly more saturated in the strip than on the stage.
    const tagged = await canvas().withIccProfile("p3").jpeg().toBuffer();
    const at = await source("p3.jpg", tagged);

    const body = await bytesOf(at, 10);

    // The bytes, not just the length: a profile of the right size is not the
    // right profile.
    const kept = (await lib(body).metadata()).icc;
    const original = (await lib(tagged).metadata()).icc;
    expect(original).toBeInstanceOf(Buffer);
    expect(kept).toBeInstanceOf(Buffer);
    expect(Buffer.compare(kept!, original!)).toBe(0);

    // And preserved, not converted: a chain that mapped the pixels toward sRGB
    // and re-tagged them P3 would be wrong the other way round and still carry
    // a matching profile.
    const before = (await lib(tagged).stats()).channels;
    const after = (await lib(body).stats()).channels;
    expect(after.map((channel) => channel.mean)).toEqual(before.map((channel) => channel.mean));
  });

  it("applies the source's EXIF orientation", async () => {
    // Orientation 6 is a quarter turn: the browser shows this 40x20 file as
    // portrait, so a thumbnail that shrinks the stored axes and drops the tag
    // comes out 10x5 — landscape, beside an upright stage picture.
    const rotated = await canvas().withMetadata({ orientation: 6 }).jpeg().toBuffer();
    const at = await source("rotated.jpg", rotated);

    const body = await bytesOf(at, 10);
    const meta = await lib(body).metadata();
    expect(meta).toMatchObject({ width: 10, height: 20 });
    // The rotation is in the pixels, so the tag must not survive as well — a
    // later `.keepExif()` would otherwise have a viewer turn the picture twice.
    expect(meta.exif).toBeUndefined();
  });

  it("flattens transparency onto white", async () => {
    // JPEG has no alpha and libvips composites over black by default, so a
    // transparent rendition reads solid black in the strip.
    const transparent = await canvas(4).png().toBuffer();
    const at = await source("transparent.png", transparent);

    const body = await bytesOf(at, 10);
    for (const channel of (await lib(body).stats()).channels) {
      expect(channel.mean).toBeGreaterThan(250);
    }
  });

  it("treats a truncated cache entry as a miss", async () => {
    // An empty `Buffer` is truthy, and `writeFile` truncates — so a server
    // killed mid-write leaves an entry that is served as a 200 marked
    // `immutable`, under a key that only changes when the source does.
    const at = await source("truncated.jpg", await canvas().jpeg().toBuffer());
    const key = thumbnailKey(at, 10);
    await mkdir(THUMBNAIL_DIR, { recursive: true });
    await writeFile(join(THUMBNAIL_DIR, key), Buffer.alloc(0));

    const body = await bytesOf(at, 10);
    expect(body.byteLength).toBeGreaterThan(0);
    // And the bad entry is replaced, so the next run does not read it either.
    expect((await readFile(join(THUMBNAIL_DIR, key))).byteLength).toBeGreaterThan(0);
  });

  it("replaces a cache entry by rename, never in place", async () => {
    // The other half of the truncation fix, and the half a reader cannot
    // observe: `writeFile` opens and truncates, so an in-place write leaves the
    // entry 0 bytes for its whole duration. Writing aside and renaming keeps the
    // entry complete at every instant — and moves the inode, which is what says
    // the file was replaced rather than opened.
    const at = await source("renamed.jpg", await canvas().jpeg().toBuffer());
    const key = thumbnailKey(at, 10);
    const entry = join(THUMBNAIL_DIR, key);
    await mkdir(THUMBNAIL_DIR, { recursive: true });
    await writeFile(entry, Buffer.alloc(0));
    const before = await stat(entry);

    expect((await bytesOf(at, 10)).byteLength).toBeGreaterThan(0);

    expect((await stat(entry)).ino).not.toBe(before.ino);
    // And nothing staged is left behind for the next run to trip over.
    const staged = (await readdir(THUMBNAIL_DIR)).filter((name) => name.startsWith(`${key}.`));
    expect(staged).toEqual([]);
  });

  it("leaves a source already inside the box alone, and caches nothing for it", async () => {
    const small = await lib({
      create: { width: 6, height: 4, channels: 3, background: "#336699" },
    })
      .jpeg()
      .toBuffer();
    const at = await source("already-small.jpg", small);

    const result = await thumbnail(at, 10);
    expect(result).toEqual({ kind: "unsuitable", why: "already within the preview width" });
    // Nothing to cache: the answer is the file, and the route serves it.
    const entries = await readdir(THUMBNAIL_DIR);
    expect(entries).not.toContain(thumbnailKey(at, 10));
  });

  it("keeps the cache directory and its entries owner-only", async () => {
    // These are derived from the user's own photographs and `tmpdir()` is the
    // shared `/tmp` on Linux, where the uid in the directory name prevents a
    // collision but grants no privacy.
    const at = await source("private.jpg", await canvas().jpeg().toBuffer());
    await bytesOf(at, 10);

    expect((await stat(THUMBNAIL_DIR)).mode & 0o077).toBe(0);
    expect((await stat(join(THUMBNAIL_DIR, thumbnailKey(at, 10)))).mode & 0o077).toBe(0);
  });

  it("still uses a cache entry for a key it once declined", async () => {
    // A transient read failure — fd exhaustion under six concurrent previews
    // plus libvips' own handles — reads as a miss and makes generation fail the
    // same way, so a decline can be recorded for a key whose entry is on disk
    // and perfectly good. Consulting the decline first would retire that
    // rendition to the full-size file for the life of the process.
    const at = await source("declined-then-cached.jpg", Buffer.from("not an image"));
    await quietly(async () => {
      expect((await thumbnail(at, 10)).kind).toBe("declined");
    });

    const good = await canvas().jpeg().toBuffer();
    await mkdir(THUMBNAIL_DIR, { recursive: true });
    await writeFile(join(THUMBNAIL_DIR, thumbnailKey(at, 10)), good);
    expect((await bytesOf(at, 10)).byteLength).toBe(good.byteLength);
  });

  it("asks sharp about a file it cannot read exactly once", async () => {
    // The frame scrolls back into view repeatedly; without a memo each pass
    // re-reads the file, re-enters libvips and prints another warning — the
    // per-view cost this module exists to remove.
    const at = await source("not-an-image.jpg", Buffer.from("plain text, despite the name"));
    const warned = await quietly(async () => {
      expect((await thumbnail(at, 10)).kind).toBe("declined");
      expect((await thumbnail(at, 10)).kind).toBe("declined");
    });
    expect(warned.length).toBe(1);
  });
});
