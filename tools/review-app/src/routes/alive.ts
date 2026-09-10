/**
 * Identity of the running server, polled by the page.
 *
 * The live stream is what makes a re-render show up promptly, but a stream that
 * dies takes the page's only news of the world with it, and a page showing the
 * previous render with no error reads exactly like a render that changed
 * nothing — the one wrong answer this tool must never give. Recovery therefore
 * rides on a plain `fetch` of this route, which cannot be affected by whatever
 * the stream is doing: a different `boot` means the server was replaced and the
 * page reloads.
 */

import { createFileRoute } from "@tanstack/solid-router";

/**
 * New on every server start. On `globalThis` because the dev server re-executes
 * module graphs, and a value that changed on an edit would reload the page for
 * no reason.
 */
const BOOT_KEY = "__ncReviewBoot";
const store = globalThis as typeof globalThis & { [BOOT_KEY]?: string };
store[BOOT_KEY] ??= `${String(Date.now())}-${Math.random().toString(36).slice(2, 10)}`;

export const Route = createFileRoute("/alive")({
  server: {
    handlers: {
      GET: () =>
        new Response(JSON.stringify({ boot: store[BOOT_KEY] }), {
          headers: { "content-type": "application/json", "cache-control": "no-store" },
        }),
    },
  },
});
