import * as stylex from "@stylexjs/stylex";
import { cls } from "./cls";

const styles = stylex.create({
  panel: { padding: 24, maxWidth: "80ch" },
  heading: { marginBlockStart: 0, fontSize: 18 },
  error: { color: "var(--bad)", whiteSpace: "pre-wrap" },
  code: {
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    fontSize: 13,
    backgroundColor: "var(--panel)",
    paddingBlock: 2,
    paddingInline: 6,
    borderRadius: 4,
  },
  block: {
    display: "block",
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    fontSize: 13,
    backgroundColor: "var(--panel)",
    padding: 12,
    borderRadius: 6,
    marginBlock: 12,
    whiteSpace: "pre-wrap",
    wordBreak: "break-all",
  },
  dim: { color: "var(--fg-dim)" },
});

/** Shown when the server could not read or parse the set it was pointed at. */
export function SetError(props: { error: unknown }) {
  return (
    <section class={cls(styles.panel)}>
      <h1 class={cls(styles.heading)}>That review set did not load</h1>
      <p class={cls(styles.error)}>{String(props.error)}</p>
      <p class={cls(styles.dim)}>
        The server reads the set from disk, so the path may be anywhere — it does not have to sit
        beside the app. Name one when starting the server:
      </p>
      <span class={cls(styles.block)}>pnpm dev ~/sets/display-tone/review.json</span>
      <p class={cls(styles.dim)}>
        or set <span class={cls(styles.code)}>REVIEW_SET</span> yourself. With neither, the bundled
        example is rendered. <span class={cls(styles.code)}>SCHEMA.md</span> documents the format.
      </p>
    </section>
  );
}
