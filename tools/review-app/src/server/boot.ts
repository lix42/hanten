/**
 * Identity of the running server process.
 *
 * The page compares this against the id baked into the document it is running,
 * so it can tell "the server restarted" from "the server is briefly
 * unreachable". Learning the baseline from the first successful poll cannot do
 * that: if the original server is already gone when the page starts polling,
 * the replacement's id becomes the baseline and the stale page never reloads.
 *
 * On `globalThis` because the dev server re-executes module graphs; a value
 * that changed on every edit would reload the page for no reason.
 */
const BOOT_KEY = "__ncReviewBoot";
const store = globalThis as typeof globalThis & { [BOOT_KEY]?: string };

export function bootId(): string {
  store[BOOT_KEY] ??= `${String(Date.now())}-${Math.random().toString(36).slice(2, 10)}`;
  return store[BOOT_KEY];
}
