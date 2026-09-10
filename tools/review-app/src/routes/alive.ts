/**
 * Identity of the running server, polled by the page.
 *
 * The live stream is what makes a re-render show up promptly, but a stream that
 * dies takes the page's only news of the world with it, and a page showing the
 * previous render with no error reads exactly like a render that changed
 * nothing — the one wrong answer this tool must never give. Recovery therefore
 * rides on a plain `fetch` of this route, which cannot be affected by whatever
 * the stream is doing: an id different from the one baked into the page means
 * the server was replaced, and the page reloads.
 */

import { createFileRoute } from "@tanstack/solid-router";
import { bootId } from "../server/boot";

export const Route = createFileRoute("/alive")({
  server: {
    handlers: {
      GET: () =>
        new Response(JSON.stringify({ boot: bootId() }), {
          headers: { "content-type": "application/json", "cache-control": "no-store" },
        }),
    },
  },
});
