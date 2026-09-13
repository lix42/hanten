import { createFileRoute } from "@tanstack/solid-router";
import { css } from "../../styled-system/css";
import { SYNTHETIC_METRICS } from "../charts/fixture";
import { parseMetrics } from "../charts/metrics";
import { MetricsPanel } from "../charts/MetricsPanel";

/**
 * The metrics panel, drawn from the committed synthetic record.
 *
 * It renders the same component the review page mounts under every picture, so
 * the two cannot drift. What it adds is the **degenerate cases a real record
 * rarely carries all at once**: a band under the sparse threshold, a band absent
 * from the cast entirely, and a channel with samples past the top of the
 * histogram's range. Those are the shapes worth looking at deliberately, and no
 * real conversion can be relied on to produce them.
 *
 * It needs no review set, no assets and no venv, so it is also how the charts
 * are looked at on a machine that has none of those.
 */

export const Route = createFileRoute("/charts")({ component: ChartsRoute });

const styles = {
  page: css.raw({ padding: "24px", display: "flex", flexDirection: "column", gap: "8px" }),
  heading: css.raw({ marginBlock: "0", fontSize: "heading" }),
  note: css.raw({ marginBlock: "0", color: "fg.dim", maxWidth: "proseMeasure" }),
};

function ChartsRoute() {
  const metrics = parseMetrics(SYNTHETIC_METRICS);
  return (
    <main class={css(styles.page)}>
      <h1 class={css(styles.heading)}>Metrics charts</h1>
      <p class={css(styles.note)}>
        The v1 component set, drawn from a synthetic record carrying the degenerate cases: a sparse
        band, a band with no pixels at all, and a channel running past the top of the range. Switch
        your system between light and dark — the chrome follows the theme, while the cast ramps do
        not, because they encode CIELAB values rather than theme decisions.
      </p>
      <MetricsPanel
        id="demo"
        configLabel="the synthetic record"
        rendition={{ src: "", preview: "", metrics }}
      />
    </main>
  );
}
