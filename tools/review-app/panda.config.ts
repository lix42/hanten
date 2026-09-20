import { defineConfig } from "@pandacss/dev";

/**
 * Panda's compiler config, and the app's whole design system.
 *
 * Styles are authored as `css()` / `css.raw()` objects in `src/`; Panda scans
 * those files, emits the atomic rules into `src/index.css`'s cascade layers, and
 * generates the typed `css` function into `styled-system/` (gitignored —
 * `panda codegen` runs from the npm scripts that need it).
 *
 * **`strictTokens` is on, so for the properties Panda checks, the theme below is
 * exhaustive**: such a measurement is a type error at the call site unless it is
 * a token here, and adding one is a deliberate edit to this file. The coverage
 * is Panda's, not ours — see the note on `strictTokens` below for the
 * properties it leaves alone. That is only useful if the token list is the
 * app's real vocabulary, which is why `presets` drops `@pandacss/preset-panda`.
 * That preset is *only* token ladders — 300 colours and rem-valued
 * spacing/size/font scales — none of which this app uses; left in, it would put
 * 300 valid-but-meaningless entries behind every autocomplete and emit them all
 * as custom properties. `@pandacss/preset-base` is the other half (357
 * utilities, 107 conditions including `_osLight`, and the patterns) and carries
 * no tokens at all, so it stays.
 *
 * Naming follows one rule: a value that denotes a *specific thing* gets a role
 * name (`sizes.thumbWidth`, `sizes.frameHeight`), and that is what retires the
 * literals this app used to repeat in two or three files. `spacing` gets no such
 * names because it has no such structure — the same 8px is a gap here, an inset
 * there and a padding elsewhere — so it is named by its measurement, and
 * `strictTokens` still gates the set.
 */
export default defineConfig({
  presets: ["@pandacss/preset-base"],

  // No reset. The app's base rules are the few in `globalCss` below; a preflight
  // would additionally restyle form controls, and the buttons here are already
  // styled from scratch.
  preflight: false,

  include: ["./src/**/*.{ts,tsx}"],
  exclude: [],

  // Every token-backed value must name a token, and every enum-valued property
  // must be a real CSS keyword. Note the coverage is Panda's, not ours: a
  // property is only checked if its utility declares a token category, which is
  // why `borderWidth`, `zIndex`, `opacity` and the SVG geometry properties
  // (`strokeWidth`, `strokeDasharray`, `fillOpacity`) still take raw values below.
  strictTokens: true,
  strictPropertyValues: true,

  outdir: "styled-system",

  // `jsxFramework` is deliberately unset: it generates a `styled.div` factory
  // and a JSX style-prop extractor, neither of which is used here. Styles are
  // named `css.raw()` objects merged at the call site, which reads better in
  // Solid than an element wrapped to carry them.

  theme: {
    // No `extend`: with preset-panda gone there is nothing to extend, and this
    // is the complete set.
    tokens: {
      colors: {
        // The one non-palette colour: the preview button's border and
        // background. Selecting it paints the border only — the background
        // stays transparent in both states.
        transparent: { value: "transparent" },
      },

      fonts: {
        body: { value: "-apple-system, system-ui, sans-serif" },
        mono: { value: "ui-monospace, SFMono-Regular, Menlo, monospace" },
      },

      // Named by role. `body` is what the page sets and what the buttons take,
      // so a control always matches surrounding text.
      fontSizes: {
        // Chart gridline labels. Smaller than any UI text on purpose: a tick
        // label is read only when the eye is already on the number beside it.
        tick: { value: "10px" },
        key: { value: "11px" }, // the <kbd> shortcut glyph
        meta: { value: "12px" }, // the source line, the fit/fullsize hint
        code: { value: "13px" }, // inline code and code blocks in SetError
        body: { value: "14px" },
        glyph: { value: "15px" }, // the pan arrows
        heading: { value: "18px" },
      },

      fontWeights: {
        semibold: { value: "600" },
      },

      lineHeights: {
        // The preview button wraps an image and must add no leading, or the
        // 2px selected border sits off-centre.
        flush: { value: "0" },
        // The two-line "missing" label inside a 70px-high box.
        snug: { value: "1.4" },
        body: { value: "1.5" },
      },

      radii: {
        sm: { value: "4px" }, // thumbnails and the mini-map inside their 6px
        // frames, and SetError's inline code chip
        md: { value: "6px" }, // buttons and preview frames
        lg: { value: "8px" }, // the viewport, pan controls, overlay notes
        round: { value: "999px" }, // a patch's delete button
      },

      // A ladder, named by measurement — see the naming rule above.
      spacing: {
        "0": { value: "0" },
        "1px": { value: "1px" },
        "2px": { value: "2px" },
        "4px": { value: "4px" },
        "5px": { value: "5px" },
        "6px": { value: "6px" },
        "8px": { value: "8px" },
        "10px": { value: "10px" },
        "12px": { value: "12px" },
        "14px": { value: "14px" },
        "16px": { value: "16px" },
        "18px": { value: "18px" },
        "20px": { value: "20px" },
        "24px": { value: "24px" },
        // Positions the pan controls across the axis they do not sit on, and
        // the "no rendition" overlay horizontally. Not a true centre — the
        // element's own edge lands on the midpoint — which is the existing
        // behaviour and deliberately unchanged here.
        half: { value: "50%" },
      },

      sizes: {
        // The preview thumbnails and the mini-map are one box, stated once:
        // three call sites used to repeat 104x70.
        thumbWidth: { value: "104px" },
        thumbHeight: { value: "70px" },
        panControl: { value: "34px" },

        // The colour readout (`i`). **Stated, not measured**: the chip flips
        // around the cursor at the pane's far edges, and that arithmetic needs
        // its size before layout — measuring it would put a read of the element
        // in the path of the write that positions it, which is the feedback
        // shape this app forbids elsewhere. So these two are a contract with
        // `ColorReadout`'s contents: change what it shows and check they still
        // hold.
        readoutWidth: { value: "212px" },
        readoutHeight: { value: "118px" },
        swatch: { value: "30px" },
        // A patch's delete button, which straddles the rectangle's corner.
        patchClose: { value: "18px" },
        // Roughly the label chip's height — it decides when the chip flips inside
        // the rectangle so the scrolling viewport cannot clip it away.
        patchChip: { value: "16px" },

        // One frame fills one screen, and these four say how that screen is
        // divided. `barHeight` is **stated, not measured**: the control bar is a
        // single non-wrapping row (its config group scrolls sideways instead), so
        // its height is a constant, and `frameHeight` can subtract it in CSS
        // rather than the page having to observe it and publish a variable.
        barHeight: { value: "52px" },
        frameHeight: { value: "calc(100dvh - {sizes.barHeight})" },
        // The charts' height, in pixels rather than a share of the viewport: the
        // band is the same on every screen, so the picture is the only thing that
        // grows with the window. A `vh` share instead gave the row three different
        // behaviours — content-sized on a tall display, capped in the middle,
        // squeezed on a short one — and made the charts biggest exactly where
        // there was already room for the picture.
        // Of the band, `MetricsPanel`'s own head and the card's title, gaps,
        // padding and caption take a fixed 121px, so the chart itself gets
        // `chartsBand - 121`.
        chartsBand: { value: "280px" },
        // And the picture's guaranteed share, which is what makes the charts the
        // side that yields. Without it flexbox took the shortfall out of the
        // picture instead: on a 609px-tall window the charts held their full
        // ceiling while the picture fell below them, inverting the whole point
        // of the split.
        pictureFloor: { value: "45vh" },

        full: { value: "100%" },

        // The note dialogs. Two widths because they hold different things: one
        // note, and one per frame in the set.
        dialogNarrow: { value: "min(680px, 92vw)" },
        dialogWide: { value: "min(900px, 94vw)" },
        dialogTall: { value: "80vh" },
        noteBox: { value: "180px" },
        noteRow: { value: "90px" },
        // The flexbox idiom for "this may shrink below its own content". A flex
        // item's `min-height` defaults to `auto`, which floors it at content
        // size — so a picture floors at the whole scan and a chart at its
        // natural height, and neither fits in a frame that is one screen tall.
        // `spacing` has a `0`, but min/max sizes read this scale.
        zero: { value: "0" },
        // Reading measures. Suffixed rather than called `prose` and `panel`,
        // because `panel` is already a *colour*: one name meaning a surface in
        // one property and a column width in another is exactly the confusion
        // naming tokens is supposed to remove.
        // These are two values for one idea — 78ch of running text under the
        // title, 80ch in the standalone panels — kept apart only because
        // unifying them would move the layout. Worth someone deciding.
        proseMeasure: { value: "78ch" },
        panelMeasure: { value: "80ch" },
        // `fullsize` means natural size, and the two keywords that say so.
        // Tokens rather than `[auto]` / `[none]`: "render at the size the file
        // is" is a real decision this app makes, and naming it is what lets the
        // escape hatch stay unused.
        natural: { value: "auto" },
        unconstrained: { value: "none" },
      },
    },

    semanticTokens: {
      // Dark by default, light under `prefers-color-scheme: light` — the same
      // shape the hand-written custom properties had. `_osLight` is an at-rule
      // condition, which is what semantic tokens require.
      colors: {
        bg: { value: { base: "#151515", _osLight: "#f6f6f6" } },
        panel: { value: { base: "#1e1e1e", _osLight: "#ffffff" } },
        edge: { value: { base: "#333333", _osLight: "#d5d5d5" } },
        fg: {
          DEFAULT: { value: { base: "#dddddd", _osLight: "#1b1b1b" } },
          dim: { value: { base: "#8f8f8f", _osLight: "#666666" } },
        },
        accent: {
          DEFAULT: { value: { base: "#7fb3ff", _osLight: "#1c62c4" } },
          // Text on top of a filled accent surface.
          ink: { value: { base: "#06131f", _osLight: "#ffffff" } },
          // The mini-map's window fill: accent, translucent enough to read the
          // panel through it.
          wash: {
            value: { base: "rgba(127, 179, 255, 0.22)", _osLight: "rgba(28, 98, 196, 0.18)" },
          },
        },
        // A patch outline. One hue on both themes, unlike every other colour
        // here, because it is drawn **over a photograph** rather than over the
        // app's own surfaces — what it has to contrast with is the picture, and
        // that is the same picture in either theme. Amber is the choice: it is
        // far from anything a neutral render produces, so an outline never reads
        // as part of the image.
        patch: {
          line: { value: "#ffc94a" },
          // The dark companion the outline is ringed with. One colour cannot sit
          // on both a blown highlight and a black shadow; two can.
          halo: { value: "rgba(0, 0, 0, 0.55)" },
          ink: { value: "#1b1200" },
          // Only the in-progress marquee is filled, and only faintly: a wash over
          // a finished patch would tint the colours it was drawn to isolate.
          wash: { value: "rgba(255, 201, 74, 0.16)" },
        },
        // A config with no rendition for this image.
        missing: { value: { base: "#40211f", _osLight: "#f6dcda" } },
        bad: { value: { base: "#ff8b7d", _osLight: "#b3261e" } },
        button: { value: { base: "#262626", _osLight: "#ffffff" } },
        // Chart series identity. The three channel colours must *look* like red,
        // green and blue — that is what a per-channel histogram is for — so hue
        // is not free to move for contrast. Red and green are consequently
        // indistinguishable under deuteranopia (measured ΔE 1.3–3.8), which no
        // palette can fix; the charts carry **dash patterns and direct labels**
        // as the secondary encoding instead. Lightness and contrast-vs-surface
        // are validated on both themes.
        series: {
          luminance: { value: { base: "#dddddd", _osLight: "#1b1b1b" } },
          r: { value: { base: "#e0685f", _osLight: "#c43b31" } },
          g: { value: { base: "#3f9e66", _osLight: "#1f7a45" } },
          b: { value: { base: "#4a8fd8", _osLight: "#1f5fb0" } },
        },
        // A gridline that has to be read against, not through: the neutral line
        // on a cast chart and mid-grey on a histogram.
        gridStrong: { value: { base: "#5a5a5a", _osLight: "#9a9a9a" } },
        // Behind the pan controls and the "no rendition" note, both of which
        // sit over the image.
        scrim: {
          value: { base: "rgba(20, 20, 20, 0.82)", _osLight: "rgba(255, 255, 255, 0.86)" },
        },
      },
    },
  },

  globalCss: {
    // Snapping lives on the scroll container, which is the document: one frame
    // fills one screen, so a scroll that lands near a boundary settles on it.
    // `proximity` rather than `mandatory` — `fullsize` scrolls *inside* a frame,
    // and a mandatory snap fights that.
    //
    // `scrollPaddingTop` is what keeps a snapped frame out from under the sticky
    // control bar: without it the frame's top edge aligns with the scrollport's,
    // which the bar covers.
    html: {
      scrollSnapType: "y proximity",
      // **Spelled as a reference, not as a token name.** `scrollPaddingTop` reads
      // the `spacing` scale, and `barHeight` is a `sizes` token, so the bare name
      // was emitted verbatim as `scroll-padding-top: barHeight` — invalid CSS,
      // dropped by the browser, and reported by nothing: `globalCss` is outside
      // `strictTokens`, so all four gates stayed green while every snapped frame
      // had its label sitting under the control bar.
      scrollPaddingTop: "{sizes.barHeight}",
    },
    ":root": {
      // Lets the UA draw form controls and scrollbars in either scheme. It does
      // not choose one: which palette applies is decided by the tokens above,
      // whose `base` values are the dark set.
      colorScheme: "light dark",
    },
    body: {
      margin: "0",
      background: "bg",
      color: "fg",
      fontFamily: "body",
      fontSize: "body",
      lineHeight: "body",
    },
  },
});
