# tools/review-app — agent notes

Read `README.md` (traps under "Notes for the next person") and `SCHEMA.md` before
editing. This file holds only what they do not.

- **Gates here are `pnpm check && pnpm test && pnpm build`**, never the Rust ones.
- **The browser is the only gate on a component** — `vp test` collects no `.tsx`
  and has no DOM. Keep logic in pure `.ts` with a sibling test.
- **Drive the app with the `agent-browser` skill; fall back to the chrome-devtools
  MCP only when it cannot do the job.** `agent-browser mouse move/down/up` sends
  real CDP input; the MCP's `evaluate_script` only dispatches synthetic events,
  which cannot exercise `setPointerCapture`, `:hover` or real hit-testing.
  - `agent-browser eval` shares one top-level scope across calls: wrap each snippet
    in an IIFE or a second `const` collides.
  - Its page runs no rendering steps, so `requestAnimationFrame` and
    `ResizeObserver` never fire even though `visibilityState` is `visible`. Any
    check of rAF- or observer-driven code there is vacuous — use the headed
    chrome-devtools MCP for those.
  - The skill file is a stub; the guide is `agent-browser skills get core`. If its
    documented commands are missing, compare `agent-browser --version` with npm.
- **Never commit a review set** — the images are the user's photographs. The only
  committed set is `public/examples/synthetic/` (generated SVG).
