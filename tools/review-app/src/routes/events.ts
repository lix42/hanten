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
        let controller: ReadableStreamDefaultController<Uint8Array> | undefined;

        const send = () => {
          try {
            controller?.enqueue(encoder.encode("event: changed\ndata: 1\n\n"));
          } catch {
            // Closed under us; cleanup has run or is about to.
          }
        };

        // Subscribed *before* the stream exists. Awaiting this inside `start`
        // left a window in which an aborted request had nothing to unsubscribe,
        // leaking the listener for the life of the process.
        const unsubscribe = await onReviewSetChange(send);
        let released = false;
        const release = () => {
          if (released) return;
          released = true;
          unsubscribe();
        };

        // Already gone. `addEventListener` on a signal that has *already*
        // aborted never fires, so without this the listener would sit in the
        // watcher's set for the life of the process.
        if (request.signal.aborted) {
          release();
          return new Response(null, { status: 499 });
        }

        const stream = new ReadableStream<Uint8Array>({
          start(c) {
            controller = c;
            // Opening comment so the client sees the connection is live.
            c.enqueue(encoder.encode(": watching\n\n"));
          },
          cancel: release,
        });

        // A navigated-away page never runs `cancel`, so drop the listener on the
        // request's own abort signal too, or every reload leaks one.
        request.signal.addEventListener("abort", release);

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
