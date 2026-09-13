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
  panel: css.raw({ marginBlockStart: "16px" }),
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
  row: css.raw({ display: "flex", flexWrap: "wrap", gap: "16px", alignItems: "flex-start" }),
  card: css.raw({
    display: "flex",
    flexDirection: "column",
    gap: "6px",
    backgroundColor: "panel",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    borderRadius: "lg",
    padding: "10px",
  }),
  cardTitle: css.raw({ fontSize: "key", fontWeight: "semibold" }),
  cardNote: css.raw({ fontSize: "tick", color: "fg.dim", maxWidth: "chartMeasure" }),
  absent: css.raw({ color: "fg.dim", fontSize: "meta" }),
  failed: css.raw({ color: "bad", fontSize: "meta", fontFamily: "mono", wordBreak: "break-word" }),
};

const CHART_WIDTH = 460;

interface Props {
  /**
   * Unique per (image, config). The cast chart's gradients are addressed by id,
   * and every section of the page is in one document — two charts sharing an id
   * would paint one curve with the other's ramp.
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
          <span class={css(styles.heading)}>Measured — {props.configLabel}</span>
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
                <Histogram
                  histogram={record().histogram}
                  series={["luminance"]}
                  width={CHART_WIDTH}
                />
                <span class={css(styles.cardNote)}>
                  One bin per L* unit. Diffuse white is the reference the record names, so how far
                  short a render stops is read rather than inferred.
                </span>
              </div>

              <div class={css(styles.card)}>
                <span class={css(styles.cardTitle)}>Per channel</span>
                <Histogram
                  histogram={record().histogram}
                  series={["luminance", "r", "g", "b"]}
                  width={CHART_WIDTH}
                />
                <span class={css(styles.cardNote)}>
                  Red and green cannot be told apart under deuteranopia, so identity rests on the
                  dash pattern and the direct label.
                </span>
              </div>

              <div class={css(styles.card)}>
                <span class={css(styles.cardTitle)}>Cast over tone</span>
                <CastOverTone cast={record().cast} id={props.id} width={CHART_WIDTH} />
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
