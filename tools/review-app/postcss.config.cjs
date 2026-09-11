// Panda runs as a PostCSS plugin, which is its recommended integration: Vite
// already pipes every CSS file through PostCSS, so `src/index.css` — the only
// stylesheet — gets the generated rules substituted into its `@layer`
// declaration in both dev and build. That is why there is no dev/build
// asymmetry to wire by hand here.
module.exports = {
  plugins: {
    "@pandacss/dev/postcss": {},
  },
};
