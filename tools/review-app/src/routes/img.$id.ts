/**
 * Serve one registered image, whole or as a thumbnail.
 *
 * Only files the review set actually named are reachable: the id addresses an
 * entry in the asset map, so a path is never taken from the URL and `..` in one
 * means nothing. The URL carries the file's mtime as `?v=`, so the response can
 * be cached indefinitely — re-rendering a frame produces a different URL rather
 * than needing a revalidation.
 *
 * `?w=` asks for the preview strip's size instead of the file. `?t=` rides
 * along and is never read here: it names the generation pipeline, so that a
 * changed chain changes the URL — without which an `immutable` response already
 * in a browser cache would outlive the fix. The thumbnail is *not* keyed on the
 * `?v=` in the URL either: it is keyed on the stat this request
 * takes, so within the window between a re-render and the watcher's reload both
 * halves of the page describe the same bytes rather than the same URL. See
 * `server/thumbs.ts` for why the strip cannot be served the full-size image.
 */

import { createFileRoute } from "@tanstack/solid-router";
import { createReadStream, statSync, type Stats } from "node:fs";
import { Readable } from "node:stream";
import { contentTypeFor } from "../server/assets";
import { currentReviewSet } from "../server/state";
import { isThumbnailable, parseThumbnailWidth, thumbnail } from "../server/thumbs";

export const Route = createFileRoute("/img/$id")({
  server: {
    handlers: {
      GET: async ({ params, request }) => {
        const set = await currentReviewSet();
        const asset = set.assets.get(params.id);
        if (!asset) return new Response("Not a registered image", { status: 404 });

        let stats: Stats;
        try {
          stats = statSync(asset.path);
        } catch {
          // A rendition the set names but that was never written. The page
          // renders the gap; saying which file is missing beats a bare 404.
          return new Response(`No such file: ${asset.path}`, { status: 404 });
        }

        // Every failure below falls through to the file itself, which is what
        // this route served before thumbnails existed: slow, never wrong.
        const width = parseThumbnailWidth(new URL(request.url).searchParams.get("w"));
        let declined = false;
        if (width !== undefined && isThumbnailable(asset.path)) {
          // Keyed on the stat this request just took, not the set's load-time
          // one: between a re-render and the watcher's reload the full-size
          // path already streams the new bytes, and the strip must not answer
          // for the same URL out of the previous render's cache entry.
          const result = await thumbnail(
            { path: asset.path, mtimeMs: stats.mtimeMs, size: stats.size },
            width,
          );
          if (result.kind === "thumbnail") {
            return new Response(new Uint8Array(result.body), {
              headers: {
                "content-type": "image/jpeg",
                "content-length": String(result.body.byteLength),
                "cache-control": "public, max-age=31536000, immutable",
              },
            });
          }
          // A failed *attempt* may succeed next time; a file that should not be
          // shrunk at all never will. Only the first restricts how long the
          // fallback below may be kept.
          declined = result.kind === "declined";
        }

        const body = Readable.toWeb(createReadStream(asset.path)) as ReadableStream<Uint8Array>;
        return new Response(body, {
          headers: {
            "content-type": contentTypeFor(asset.path),
            "content-length": String(stats.size),
            // **A declined generation must not be cached like a decision.** The
            // usual `immutable` is a statement about the *file*, which is right
            // for one nothing will ever shrink — an SVG, a format off the list.
            // A decline is a statement about one attempt: a moment of fd
            // exhaustion, a temp dir that was read-only. Marking that
            // `immutable` would pin the 7 MB original under the *preview* URL
            // for a year, so a single transient failure would retire that
            // frame's thumbnail until a hard reload — and the server-side
            // recovery (`declined` is per process, and the disk cache is read
            // first) could never reach the browser.
            "cache-control": declined
              ? "public, max-age=60"
              : "public, max-age=31536000, immutable",
          },
        });
      },
    },
  },
});
