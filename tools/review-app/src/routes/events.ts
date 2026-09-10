/**
 * Server-sent events telling the page its review set changed.
 *
 * One message per settled change; the page answers it by invalidating its
 * loader, which re-reads the model and picks up every rendition's new `/img/`
 * URL. `EventSource` reconnects by itself, so a dev-server restart re-arms this
 * without the page having to do anything.
 */

import { createFileRoute } from "@tanstack/solid-router";
import { onReviewSetChange } from "../server/watcher";

export const Route = createFileRoute("/events")({
  server: {
    handlers: {
      GET: async ({ request }) => {
        const encoder = new TextEncoder();
        let unsubscribe: (() => void) | undefined;

        const stream = new ReadableStream<Uint8Array>({
          async start(controller) {
            const send = () => {
              try {
                controller.enqueue(encoder.encode("event: changed\ndata: 1\n\n"));
              } catch {
                // Closed under us; the abort below does the cleanup.
              }
            };
            unsubscribe = await onReviewSetChange(send);
            // Opening comment so the client sees the connection is live.
            controller.enqueue(encoder.encode(": watching\n\n"));
          },
          cancel() {
            unsubscribe?.();
          },
        });

        // A navigated-away page never runs `cancel`, so drop the listener on the
        // request's own abort signal too, or every reload leaks one.
        request.signal.addEventListener("abort", () => unsubscribe?.());

        return new Response(stream, {
          headers: {
            "content-type": "text/event-stream",
            "cache-control": "no-cache",
            connection: "keep-alive",
          },
        });
      },
    },
  },
});
