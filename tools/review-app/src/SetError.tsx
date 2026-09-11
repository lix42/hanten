import { css } from "../styled-system/css";

const styles = {
  panel: css.raw({ padding: "24px", maxWidth: "panelMeasure" }),
  heading: css.raw({ marginBlockStart: "0", fontSize: "heading" }),
  error: css.raw({ color: "bad", whiteSpace: "pre-wrap" }),
  code: css.raw({
    fontFamily: "mono",
    fontSize: "code",
    backgroundColor: "panel",
    paddingBlock: "2px",
    paddingInline: "6px",
    borderRadius: "sm",
  }),
  block: css.raw({
    display: "block",
    fontFamily: "mono",
    fontSize: "code",
    backgroundColor: "panel",
    padding: "12px",
    borderRadius: "md",
    marginBlock: "12px",
    whiteSpace: "pre-wrap",
    wordBreak: "break-all",
  }),
  dim: css.raw({ color: "fg.dim" }),
};

/** Shown when the server could not read or parse the set it was pointed at. */
export function SetError(props: { error: unknown }) {
  return (
    <section class={css(styles.panel)}>
      <h1 class={css(styles.heading)}>That review set did not load</h1>
      <p class={css(styles.error)}>{String(props.error)}</p>
      <p class={css(styles.dim)}>
        The server reads the set from disk, so the path may be anywhere — it does not have to sit
        beside the app. Name one when starting the server:
      </p>
      <span class={css(styles.block)}>pnpm dev ~/sets/display-tone/review.json</span>
      <p class={css(styles.dim)}>
        or set <span class={css(styles.code)}>REVIEW_SET</span> yourself. With neither, the bundled
        example is rendered. <span class={css(styles.code)}>SCHEMA.md</span> documents the format.
      </p>
    </section>
  );
}
