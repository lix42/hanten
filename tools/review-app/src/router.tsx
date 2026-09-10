import { createRouter as createTanStackRouter } from "@tanstack/solid-router";
import { routeTree } from "./routeTree.gen";

export function getRouter() {
  return createTanStackRouter({ routeTree, scrollRestoration: true });
}

declare module "@tanstack/solid-router" {
  interface Register {
    router: ReturnType<typeof getRouter>;
  }
}
