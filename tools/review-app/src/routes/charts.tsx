import { createFileRoute } from "@tanstack/solid-router";
import { css } from "../../styled-system/css";
import { CastOverTone } from "../charts/CastOverTone";
import { SYNTHETIC_METRICS } from "../charts/fixture";
import { Histogram } from "../charts/Histogram";
import { parseMetrics } from "../charts/metrics";

/**
 * The v1 metrics charts, rendered from a committed synthetic record.
 *
 * A place to look at the components while they have no data source — how a real
 * metrics record reaches the page belongs to `analysis/metrics-visualization`,
 * which is a separate task. Nothing here touches `review.json`, the server or the
 * watcher, and this route is expected to be retired or repurposed when that half
 * lands.
 *
 * The fixture is held in the raw `snake_case` shape and parsed here, so the demo
 * exercises `parseMetrics` rather than going around it.
 */

export const Route = createFileRoute("/charts")({ component: ChartsRoute });

const styles = {
  page: css.raw({ padding: "24px", display: "flex", flexDirection: "column", gap: "24px" }),
  heading: css.raw({ marginBlock: "0", fontSize: "heading" }),
  note: css.raw({ color: "fg.dim", maxWidth: "proseMeasure", marginBlock: "0" }),
  row: css.raw({ display: "flex", flexWrap: "wrap", gap: "20px", alignItems: "flex-start" }),
  card: css.raw({
    display: "flex",
    flexDirection: "column",
    gap: "8px",
    backgroundColor: "panel",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    borderRadius: "lg",
    padding: "12px",
  }),
  cardTitle: css.raw({ fontSize: "key", fontWeight: "semibold" }),
  cardNote: css.raw({ fontSize: "tick", color: "fg.dim", maxWidth: "full" }),
};

function ChartsRoute() {
  const metrics = parseMetrics(SYNTHETIC_METRICS);
  return (
    <main class={css(styles.page)}>
      <div>
        <h1 class={css(styles.heading)}>Metrics charts</h1>
        <p class={css(styles.note)}>
          The v1 component set, drawn from a synthetic record. Switch your system between light and
          dark: the chrome follows the theme, while the cast ramps do not — they encode CIELAB
          values, not theme decisions.
        </p>
      </div>

      <div class={css(styles.row)}>
        <div class={css(styles.card)}>
          <span class={css(styles.cardTitle)}>Luminance histogram</span>
          <Histogram histogram={metrics.histogram} series={["luminance"]} />
          <span class={css(styles.cardNote)}>
            One bin per L* unit, as the record stores it. Diffuse white is the reference the record
            names, so how far short a render stops is read rather than inferred.
          </span>
        </div>

        <div class={css(styles.card)}>
          <span class={css(styles.cardTitle)}>Per-channel histogram</span>
          <Histogram histogram={metrics.histogram} series={["luminance", "r", "g", "b"]} />
          <span class={css(styles.cardNote)}>
            Red and green cannot be told apart under deuteranopia, so identity rests on the dash
            pattern and the direct label, with colour as the redundant cue.
          </span>
        </div>
      </div>

      <div class={css(styles.row)}>
        <div class={css(styles.card)}>
          <span class={css(styles.cardTitle)}>Cast over tone</span>
          <CastOverTone cast={metrics.cast} id="demo" width={560} />
          <span class={css(styles.cardNote)}>
            Each line is coloured by its own value, so it teaches its own axis. Marker area is the
            band&apos;s share of the measured region; the hollow marker is a band under{" "}
            {metrics.bands.sparseBelowFraction * 100}% of the region, which is not a measurement.
          </span>
        </div>
      </div>
    </main>
  );
}
