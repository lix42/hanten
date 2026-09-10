import stylex from "@stylexjs/unplugin";
import { tanstackStart } from "@tanstack/solid-start/plugin/vite";
import solid from "vite-plugin-solid";
import { defineConfig, lazyPlugins } from "vite-plus";

export default defineConfig(({ mode }) => ({
  plugins:
    // The unit tests cover pure modules — no styles, no routes, no server. Both
    // the StyleX compiler and TanStack Start's route generation would have
    // nothing to do there, and StyleX's plugin holds a handle that stops Vitest
    // from exiting (measured: 10.9s with it, 0.9s without).
    mode === "test"
      ? [solid()]
      : lazyPlugins(() => [
          // StyleX must come *before* the framework plugins, or Fast Refresh breaks.
          // `devPersistToDisk` shares the collected rules across Vite's client and
          // ssr environments, which SSR splits the app into.
          stylex.vite({ useCSSLayers: true, devPersistToDisk: true }),
          tanstackStart(),
          solid({ ssr: true }),
        ]),
  // Lint and format settings live here rather than in separate oxlint/oxfmt
  // files — Vite+ reads them from the one config, and `vp check` runs all three
  // (format, lint, type-check) as a single gate.
  lint: {
    // `routeTree.gen.ts` is written by TanStack's route generator on every dev
    // and build run; linting it only reports on generated code.
    ignorePatterns: ["dist/**", ".output/**", ".tanstack/**", "src/routeTree.gen.ts"],
    options: { typeAware: true, typeCheck: true },
  },
  fmt: {
    ignorePatterns: ["dist/**", ".output/**", ".tanstack/**", "src/routeTree.gen.ts"],
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
}));
