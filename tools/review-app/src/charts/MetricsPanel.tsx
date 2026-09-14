import { Show } from "solid-js";
import { css } from "../../styled-system/css";
import type { Rendition } from "../review";
import { CastOverTone } from "./CastOverTone";
import { Histogram } from "./Histogram";

/**
 * The measurement of the rendition currently on screen.
 *
 * Bound to the active config and swapped in place exactly as the picture is:
 * all three v1 charts spend colour on what they encode — channel identity, or
 * the sign of `a*`/`b*` — so none of them has a series dimension left to carry a
 * second configuration. Comparing means switching config and watching the shape
 * change, which is the same gesture the picture above already asks for.
 *
 * Charts are never *partly* drawn: a rendition with no record says so, and one
 * whose record could not be read says why. Both leave the picture alone, which
 * is the thing this app exists to show.
 */

const styles = {
  // Takes its height from whoever mounts it: inside a frame that is a capped
  // band, on `/charts` it is normal flow and the charts draw at natural size.
  // `minHeight: 0` is what lets the row below actually shrink inside a flex
  // parent — without it a flex item floors at its content height and the cap is
  // ignored.
  panel: css.raw({
    marginBlockStart: "16px",
    display: "flex",
    flexDirection: "column",
    minHeight: "zero",
    maxHeight: "full",
  }),
  head: css.raw({
    display: "flex",
    gap: "10px",
    alignItems: "baseline",
    flexWrap: "wrap",
    marginBottom: "8px",
  }),
  heading: css.raw({ fontSize: "key", fontWeight: "semibold" }),
  scope: css.raw({ color: "fg.dim", fontSize: "meta", fontVariantNumeric: "tabular-nums" }),
  caveat: css.raw({ color: "accent", fontSize: "meta" }),
  // Three equal columns that never wrap. A grid rather than a wrapping flex row
  // because the three charts are one instrument read together — wrapping put the
  // cast chart on its own line at some widths, and comparing a shape against a
  // shape below it is not the same gesture as comparing it against one beside it.
  row: css.raw({
    display: "grid",
    gridTemplateColumns: "repeat(3, 1fr)",
    gap: "16px",
    alignItems: "stretch",
    minHeight: "zero",
    flexGrow: 1,
  }),
  card: css.raw({
    display: "flex",
    flexDirection: "column",
    gap: "6px",
    minHeight: "zero",
    backgroundColor: "panel",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    borderRadius: "lg",
    padding: "10px",
  }),
  cardTitle: css.raw({ fontSize: "key", fontWeight: "semibold", flexShrink: 0 }),
  // The caption's measure is the column it sits in, now that the three cards are
  // equal grid tracks; it used to be pinned to a fixed chart width.
  //
  // **Dropped outright on a short viewport rather than shrunk.** Inside a frame
  // the charts live in a capped band, and three captions cost more of it than
  // the charts themselves: left in at 609px the caption held its full height and
  // squeezed the chart to 7px — a caption explaining a drawing that was no longer
  // there. A chart without its caption is still a chart. `/charts` keeps its
  // captions at any height — not because it is uncapped, but because the rule
  // below is scoped to the band; an unscoped one took them from that page too.
  cardNote: css.raw({
    fontSize: "tick",
    color: "fg.dim",
    flexShrink: 0,
    // **Scoped to the band, not to the viewport.** An unscoped media query is a
    // global rule, so it stripped the captions from `/charts` too — the one page
    // that exists to read the charts, and which has no cap to be squeezed by.
    "@media (max-height: 820px)": { "[data-charts-band] &": { display: "none" } },
  }),
  absent: css.raw({ color: "fg.dim", fontSize: "meta" }),
  failed: css.raw({ color: "bad", fontSize: "meta", fontFamily: "mono", wordBreak: "break-word" }),
};

/**
 * One box for all three charts, so they render at identical size.
 *
 * It is a **coordinate system, not a size**: each `<svg>` carries this as its
 * `viewBox` and is laid out at whatever its grid column gives it, so the numbers
 * below fix the shared aspect ratio and nothing else. They did not used to
 * match — the cast chart was 460x330 against the histograms' 460x300 — which
 * made it visibly taller at equal width, in a row whose whole point is that the
 * three are comparable.
 */
const CHART_BOX = { width: 460, height: 300 } as const;

interface Props {
  /**
   * Unique per (image, config), and **DOM-safe** — build it with `domId`.
   *
   * The cast chart's gradients are addressed by this id, and every section of
   * the page is in one document, so two charts sharing one would paint a curve
   * with the other's ramp. A review set's own ids cannot be used raw: the schema
   * permits any non-empty string, and one holding `)` closes the paint URL.
   */
  id: string;
  rendition: Rendition | undefined;
  configLabel: string;
}

export function MetricsPanel(props: Props) {
  const metrics = () => props.rendition?.metrics;
  /**
   * How much of the frame the numbers describe, when it is not all of it.
   *
   * "Central" only when the margins actually are equal: a review set insets
   * evenly, but a record measured with `--region` can describe a corner, and
   * calling that central would misdescribe every chart below.
   */
  const scope = () => {
    const region = metrics()?.region;
    if (!region) return undefined;
    const share = region.fractionWidth * region.fractionHeight;
    if (share >= 0.999) return "the whole frame";
    const centred =
      Math.abs(region.fractionX * 2 + region.fractionWidth - 1) < 0.005 &&
      Math.abs(region.fractionY * 2 + region.fractionHeight - 1) < 0.005;
    return `${centred ? "the central " : ""}${(share * 100).toFixed(0)}% of the frame`;
  };

  return (
    <Show when={props.rendition}>
      <div class={css(styles.panel)}>
        <div class={css(styles.head)}>
          {/* "Measured" only when something was: the heading sat above the
              "no measurement" line and contradicted it, which is the common
              state for a set rendered with `--no-metrics`. */}
          <span class={css(styles.heading)}>
            {metrics()
              ? "Measured"
              : props.rendition?.metricsError
                ? "Measurement"
                : "Not measured"}{" "}
            — {props.configLabel}
          </span>
          <Show when={scope()}>
            {(what) => (
              <span class={css(styles.scope)}>
                {what()}, {((metrics()?.region.pixels ?? 0) / 1e6).toFixed(1)} Mpx
              </span>
            )}
          </Show>
          {/* The picture above is the file itself, which an HDR display decodes
              as the HDR rendition; the numbers are the SDR base, because that is
              what `nctool metrics` can read. Said out loud rather than left for
              the reader to discover the two disagree. */}
          <Show when={metrics()?.source.gainMapPresent}>
            <span class={css(styles.caveat)}>
              {metrics()?.source.jpegImage === "hdr" ? "HDR rendition" : "SDR base"} of a gain-map
              file — the picture above may be shown in HDR
            </span>
          </Show>
        </div>

        <Show
          when={metrics()}
          fallback={
            <Show
              when={props.rendition?.metricsError}
              fallback={
                <div class={css(styles.absent)}>
                  No measurement for this rendition. Render the set with{" "}
                  <code>nctool review generate</code>, or measure it with{" "}
                  <code>nctool metrics image</code>.
                </div>
              }
            >
              {(why) => <div class={css(styles.failed)}>Measurement unreadable — {why()}</div>}
            </Show>
          }
        >
          {(record) => (
            <div class={css(styles.row)}>
              <div class={css(styles.card)}>
                <span class={css(styles.cardTitle)}>Tone</span>
                <Histogram histogram={record().histogram} series={["luminance"]} {...CHART_BOX} />
                <span class={css(styles.cardNote)}>
                  One bin per L* unit. Diffuse white is the reference the record names, so how far
                  short a render stops is read rather than inferred.
                </span>
              </div>

              <div class={css(styles.card)}>
                <span class={css(styles.cardTitle)}>Per channel</span>
                {/* The three channels only: the luminance curve this used to
                    carry is the Tone chart beside it, drawn twice. Dropping it
                    also lets the y axis follow the channels, which previously
                    shared a ceiling with a series that is not plotted here. */}
                <Histogram histogram={record().histogram} series={["r", "g", "b"]} {...CHART_BOX} />
                <span class={css(styles.cardNote)}>
                  Red and green cannot be told apart under deuteranopia, so identity rests on the
                  dash pattern and the direct label.
                </span>
              </div>

              <div class={css(styles.card)}>
                <span class={css(styles.cardTitle)}>Cast over tone</span>
                <CastOverTone cast={record().cast} id={props.id} {...CHART_BOX} />
                <span class={css(styles.cardNote)}>
                  Each line is coloured by its own value, so it teaches its own axis. Marker area is
                  the band&apos;s share of the region; a hollow marker is a band too small to be a
                  measurement.
                </span>
              </div>
            </div>
          )}
        </Show>
      </div>
    </Show>
  );
}
