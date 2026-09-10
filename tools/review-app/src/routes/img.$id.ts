/**
 * Serve one registered image.
 *
 * Only files the review set actually named are reachable: the id addresses an
 * entry in the asset map, so a path is never taken from the URL and `..` in one
 * means nothing. The URL carries the file's mtime as `?v=`, so the response can
 * be cached indefinitely — re-rendering a frame produces a different URL rather
 * than needing a revalidation.
 */

import { createFileRoute } from "@tanstack/solid-router";
import { createReadStream, statSync } from "node:fs";
import { Readable } from "node:stream";
import { contentTypeFor } from "../server/assets";
import { currentReviewSet } from "../server/state";

export const Route = createFileRoute("/img/$id")({
  server: {
    handlers: {
      GET: async ({ params }) => {
        const set = await currentReviewSet();
        const asset = set.assets.get(params.id);
        if (!asset) return new Response("Not a registered image", { status: 404 });

        let size: number;
        try {
          size = statSync(asset.path).size;
        } catch {
          // A rendition the set names but that was never written. The page
          // renders the gap; saying which file is missing beats a bare 404.
          return new Response(`No such file: ${asset.path}`, { status: 404 });
        }

        const body = Readable.toWeb(createReadStream(asset.path)) as ReadableStream<Uint8Array>;
        return new Response(body, {
          headers: {
            "content-type": contentTypeFor(asset.path),
            "content-length": String(size),
            "cache-control": "public, max-age=31536000, immutable",
          },
        });
      },
    },
  },
});
