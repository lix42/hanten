/**
 * Serve one labelling frame's render, as the browser can show it.
 *
 * Only frames the session lists are reachable — the stem is looked up, never used
 * as a path. A TIFF render (what `hanten convert` writes by default) is converted to
 * JPEG once per version and kept in memory; its ICC profile is kept, so a Display P3
 * render shows as it would in the review page.
 */

import { createFileRoute } from "@tanstack/solid-router";
import { readFileSync, statSync } from "node:fs";
import { contentTypeFor } from "../server/assets";
import { labelConfig, loadFrames } from "../server/labelSession";

const converted = new Map<string, Buffer>();

async function asJpeg(path: string, mtimeMs: number): Promise<Buffer> {
  const key = `${path}\n${String(mtimeMs)}`;
  const hit = converted.get(key);
  if (hit) return hit;
  const { default: sharp } = await import("sharp");
  const body = await sharp(path, { failOn: "none" })
    .jpeg({ quality: 92 })
    .keepIccProfile()
    .toBuffer();
  converted.set(key, body);
  return body;
}

export const Route = createFileRoute("/label-img/$stem")({
  server: {
    handlers: {
      GET: async ({ params }) => {
        const config = labelConfig(process.env);
        if (typeof config === "string") return new Response(config, { status: 404 });
        const frames = loadFrames(config);
        const frame =
          typeof frames === "string" ? undefined : frames.find((f) => f.stem === params.stem);
        if (!frame?.render) return new Response("Not a labelling frame", { status: 404 });
        const { mtimeMs } = statSync(frame.render);
        const tiff = /\.tiff?$/i.test(frame.render);
        const body = tiff ? await asJpeg(frame.render, mtimeMs) : readFileSync(frame.render);
        return new Response(new Uint8Array(body), {
          headers: {
            "content-type": tiff ? "image/jpeg" : contentTypeFor(frame.render),
            "cache-control": "public, max-age=31536000, immutable",
          },
        });
      },
    },
  },
});
