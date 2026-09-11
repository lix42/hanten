import { tanstackStart } from "@tanstack/solid-start/plugin/vite";
import solid from "vite-plugin-solid";
import { defineConfig, lazyPlugins } from "vite-plus";

export default defineConfig(({ mode }) => ({
  plugins:
    // The unit tests cover pure modules — no styles, no routes, no server — so
    // TanStack Start's route generation would have nothing to do there.
    // (Panda needs no Vite plugin at all: it runs from `postcss.config.cjs`,
    // and PostCSS only runs when a stylesheet is processed.)
    mode === "test" ? [solid()] : lazyPlugins(() => [tanstackStart(), solid({ ssr: true })]),
  // Lint and format settings live here rather than in separate oxlint/oxfmt
  // files — Vite+ reads them from the one config, and `vp check` runs all three
  // (format, lint, type-check) as a single gate.
  lint: {
    // `routeTree.gen.ts` is written by TanStack's route generator on every dev
    // and build run, and `styled-system/` by `panda codegen`; linting either
    // only reports on generated code.
    ignorePatterns: [
      "dist/**",
      ".output/**",
      ".tanstack/**",
      "styled-system/**",
      "src/routeTree.gen.ts",
    ],
    options: { typeAware: true, typeCheck: true },
  },
  fmt: {
    ignorePatterns: [
      "dist/**",
      ".output/**",
      ".tanstack/**",
      "styled-system/**",
      "src/routeTree.gen.ts",
    ],
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
}));
