import { onMount } from "solid-js";

/**
 * Start StyleX's dev-only CSS hot-reload runtime.
 *
 * The unplugin injects this by itself only when Vite's entry is an HTML file.
 * Start's entry is the route shell instead, so the runtime has to be imported
 * by hand — and it has to happen *here*, inside the routed tree, because
 * `shellComponent` renders on the server only and never reaches the browser.
 *
 * It is imported from this shim rather than loaded as
 * `<script src="/@id/virtual:stylex:runtime">`, which the unplugin's README
 * warns can be blocked when a framework proxies dev assets — as Start does.
 * The module does not exist in a build, so the import stays behind `DEV`.
 */
export function StyleXDevRuntime() {
  onMount(() => {
    if (import.meta.env.DEV) void import("virtual:stylex:runtime");
  });
  return null;
}
