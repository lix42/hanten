import { useRouter } from "@tanstack/solid-router";
import { onCleanup, onMount } from "solid-js";

/** How long to wait before replacing a connection the browser gave up on. */
const RECONNECT_MS = 1000;

/** How often to check the server is still the same one. */
const POLL_MS = 3000;

/**
 * Re-read the review set when the server says it changed.
 *
 * `invalidate` re-runs the route loader, so the page picks up a rewritten
 * `review.json` and, because every `/img/` URL carries its file's mtime, swaps
 * exactly the renditions that were re-rendered. Nothing reloads the document,
 * so the selected config and the scroll position survive.
 *
 * **Recovery does not ride on the stream.** `EventSource` retries some failures
 * itself and permanently closes on others, and neither its own retry nor a
 * hand-rolled replacement reliably brought the connection back after the dev
 * server was replaced — measured: the page went on showing the previous render,
 * with no error anywhere, which reads exactly like a re-render that changed
 * nothing. So the stream is treated as the fast path only, and correctness
 * rests on a plain poll of `/alive`: a server that comes back with a different
 * boot id gets a reload. Reloading costs the selected config and the scroll
 * position, in a case where the dev server was restarted anyway.
 */
export function LiveReload() {
  const router = useRouter();

  onMount(() => {
    let source: EventSource | undefined;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;

    const reread = () => void router.invalidate();

    const connect = () => {
      if (stopped) return;
      const stream = new EventSource("/events");
      source = stream;
      stream.addEventListener("changed", reread);
      stream.addEventListener("error", () => {
        // Every error is treated as the connection being gone, rather than
        // leaving `EventSource` to retry: measured against a dev server that
        // went away and came back, its own retry never reconnected and no
        // further event ever arrived, so deferring to it is what made the page
        // go quietly stale.
        stream.close();
        clearTimeout(retry);
        retry = setTimeout(connect, RECONNECT_MS);
      });
    };

    // Independent of the stream: a tab that was hidden while a render landed
    // catches up the moment it is looked at again.
    const onVisible = () => {
      if (document.visibilityState === "visible") reread();
    };
    document.addEventListener("visibilitychange", onVisible);

    // Independent of the stream, and of the router: if the server is a different
    // one than the page was rendered against, only a reload is trustworthy.
    let boot: string | undefined;
    const poll = setInterval(() => {
      void fetch("/alive", { cache: "no-store" })
        .then((response) => (response.ok ? (response.json() as Promise<{ boot?: string }>) : null))
        .then((body) => {
          if (!body?.boot) return;
          if (boot !== undefined && boot !== body.boot) window.location.reload();
          boot = body.boot;
        })
        .catch(() => undefined); // server down; the next tick tries again
    }, POLL_MS);

    connect();
    onCleanup(() => {
      clearInterval(poll);
      stopped = true;
      clearTimeout(retry);
      source?.close();
      document.removeEventListener("visibilitychange", onVisible);
    });
  });

  return null;
}
