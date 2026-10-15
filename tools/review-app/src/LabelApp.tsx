/**
 * The chart-labelling page (`pnpm label`): place each target's four corners on a
 * frame, see the projected patches, and save — which writes the labels file and runs
 * the placement check on the scan. The arithmetic is in `chart.ts`; the session and
 * the check in `server/labelSession.ts`.
 */

import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { css } from "../styled-system/css";
import {
  chooseByAnchor,
  fitLongSide,
  orderClockwise,
  failingIds,
  projectPatches,
  rotate,
  type ChartLabel,
  type CheckResult,
  type Point,
  type Quad,
  type Target,
} from "./chart";
import { saveLabels, type LabelSessionPayload } from "./server/labels";

type Session = Extract<LabelSessionPayload, { ok: true }>;

// `css.raw` where a style is merged at the call site (`css(a, b)`): two atomic
// classes setting one property leave the winner to stylesheet order.
const styles = {
  page: css({ display: "flex", flexDirection: "column", height: "labelPage" }),
  bar: css({
    display: "flex",
    flexWrap: "wrap",
    gap: "8px",
    alignItems: "center",
    paddingBlock: "8px",
    paddingInline: "16px",
    flexShrink: 0,
    backgroundColor: "panel",
    borderBottomWidth: "1px",
    borderBottomStyle: "solid",
    borderBottomColor: "edge",
  }),
  title: css({ fontSize: "heading", fontWeight: "semibold", marginInlineEnd: "12px" }),
  body: css({ display: "flex", flexGrow: 1, minHeight: "zero" }),
  side: css({
    width: "labelSide",
    flexShrink: 0,
    overflowY: "auto",
    borderRightWidth: "1px",
    borderRightStyle: "solid",
    borderRightColor: "edge",
  }),
  frameButton: css.raw({
    display: "block",
    width: "full",
    textAlign: "left",
    paddingBlock: "6px",
    paddingInline: "10px",
    borderWidth: "0",
    backgroundColor: "transparent",
    color: "fg",
    fontFamily: "body",
    fontSize: "body",
    cursor: "pointer",
  }),
  frameActive: css.raw({ backgroundColor: "accent", color: "accent.ink" }),
  stage: css({ flexGrow: 1, overflow: "auto", minWidth: "zero", padding: "8px" }),
  holder: css({ position: "relative", display: "inline-block", lineHeight: "flush" }),
  imageFit: css({ maxWidth: "full", maxHeight: "labelImage", display: "block" }),
  imageNatural: css({ maxWidth: "unconstrained", display: "block" }),
  overlay: css({ position: "absolute", top: "0", left: "0", width: "full", height: "full" }),
  panel: css({
    width: "labelPanel",
    flexShrink: 0,
    overflowY: "auto",
    padding: "12px",
    borderLeftWidth: "1px",
    borderLeftStyle: "solid",
    borderLeftColor: "edge",
    fontSize: "meta",
  }),
  button: css.raw({
    paddingBlock: "5px",
    paddingInline: "12px",
    borderRadius: "md",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    backgroundColor: "button",
    color: "fg",
    fontFamily: "body",
    fontSize: "body",
    cursor: "pointer",
  }),
  active: css.raw({ backgroundColor: "accent", borderColor: "accent", color: "accent.ink" }),
  dim: css({ color: "fg.dim" }),
  bad: css({ color: "bad" }),
  prompt: css({ color: "patch.line", fontWeight: "semibold" }),
  input: css({
    width: "thumbWidth",
    paddingBlock: "4px",
    paddingInline: "6px",
    fontFamily: "body",
    fontSize: "body",
    backgroundColor: "button",
    color: "fg",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    borderRadius: "sm",
  }),
};

const copyLabel = (l: ChartLabel): ChartLabel => ({
  target: l.target,
  card: l.card,
  corners: l.corners.map((p) => [p[0], p[1]] as const) as unknown as Quad,
});

export function LabelApp(props: { session: LabelSessionPayload }) {
  return (
    <Show
      when={props.session.ok ? props.session : undefined}
      fallback={
        <p class={css({ padding: "24px" })}>{props.session.ok ? "" : props.session.error}</p>
      }
    >
      {(session) => <Labeller session={session()} />}
    </Show>
  );
}

function Labeller(props: { session: Session }) {
  const s = props.session;
  const [index, setIndex] = createSignal(0);
  const [labels, setLabels] = createStore<Record<string, ChartLabel[]>>(
    structuredClone(s.labels) as Record<string, ChartLabel[]>,
  );
  const [results, setResults] = createStore<Record<string, readonly CheckResult[]>>({});
  const [dirty, setDirty] = createStore<Record<string, boolean>>({});
  const [seededFrom, setSeededFrom] = createStore<Record<string, string>>({});
  const [targetId, setTargetId] = createSignal(s.targets[0]?.id ?? "");
  const [placing, setPlacing] = createSignal<Point[]>([]);
  const [natural, setNatural] = createSignal(false);
  const [card, setCard] = createSignal(s.card);
  const [status, setStatus] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  let svg: SVGSVGElement | undefined;

  const frame = createMemo(() => s.frames[index()]!);
  const target = createMemo(() => s.targets.find((t) => t.id === targetId())!);
  const frameLabels = () => labels[frame().stem] ?? [];
  const labelOf = (t: string) => frameLabels().find((l) => l.target === t);

  // A bracket on a tripod: an unlabelled frame starts from the previous one's corners.
  createEffect(() => {
    const i = index();
    const stem = s.frames[i]!.stem;
    const prev = s.frames[i - 1]?.stem;
    if ((labels[stem]?.length ?? 0) === 0 && prev && (labels[prev]?.length ?? 0) > 0) {
      // A plain copy: `structuredClone` cannot clone a store's proxy.
      setLabels(stem, labels[prev]!.map(copyLabel));
      setSeededFrom(stem, prev);
      setDirty(stem, true);
    }
    setPlacing([]);
  });

  const setCorners = (corners: Quad, t: string = targetId()) => {
    const stem = frame().stem;
    setLabels(
      produce((all) => {
        const list = all[stem] ?? [];
        const i = list.findIndex((l) => l.target === t);
        const next = { target: t, card: list[i]?.card ?? card(), corners };
        all[stem] = i >= 0 ? list.map((l, k) => (k === i ? next : l)) : [...list, next];
      }),
    );
    setDirty(stem, true);
  };

  const clearTarget = () => {
    const stem = frame().stem;
    setLabels(
      stem,
      frameLabels().filter((l) => l.target !== targetId()),
    );
    setDirty(stem, true);
    setPlacing([]);
  };

  const toFraction = (e: PointerEvent): Point => {
    const box = svg!.getBoundingClientRect();
    return [
      Math.min(Math.max((e.clientX - box.left) / box.width, 0), 1),
      Math.min(Math.max((e.clientY - box.top) / box.height, 0), 1),
    ];
  };

  // A saved check that found several passing ways up and is waiting for the user to
  // say which, by clicking the target's anchor patch.
  const pending = () => {
    if (dirty[frame().stem]) return undefined;
    const r = (results[frame().stem] ?? []).find((x) => (x.candidates?.length ?? 0) > 1);
    const t = r && s.targets.find((x) => x.id === r.target);
    return r && t ? { result: r, target: t } : undefined;
  };

  const place = (e: PointerEvent) => {
    const p = pending();
    if (p) {
      const chosen = chooseByAnchor(p.target, p.result.candidates ?? [], toFraction(e));
      if (!chosen) {
        setStatus(`that is not ${p.target.anchor.prompt} in any way up that passed; try again`);
        return;
      }
      setCorners(chosen, p.target.id);
      void save();
      return;
    }
    if (labelOf(targetId())) return;
    const next = [...placing(), toFraction(e)];
    if (next.length === 4) {
      // Any click order: sorted round the outline and laid along its long side; the
      // check on save settles which end is the top.
      const f = frame();
      setCorners(fitLongSide(target(), orderClockwise(next, f.width, f.height), f.width, f.height));
      setPlacing([]);
    } else setPlacing(next);
  };

  const startDrag = (t: string, corner: number, e: PointerEvent) => {
    e.stopPropagation();
    setTargetId(t);
    const el = e.currentTarget as Element;
    el.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const label = labelOf(t);
      if (!label) return;
      const corners = [...label.corners] as Point[];
      corners[corner] = toFraction(ev);
      setCorners(corners as unknown as Quad);
    };
    const up = () => {
      el.removeEventListener("pointermove", move as EventListener);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move as EventListener);
    el.addEventListener("pointerup", up);
  };

  const save = async () => {
    const stem = frame().stem;
    setBusy(true);
    setStatus(`checking ${stem} against its scan…`);
    try {
      const r = await saveLabels({ data: { stem, labels: frameLabels() } });
      if (r.ok) {
        setResults(stem, r.results);
        // The check may have turned a label to the orientation that passed.
        setLabels(
          stem,
          frameLabels().map((l, i) => ({ ...l, corners: r.results[i]?.corners ?? l.corners })),
        );
        setDirty(stem, false);
        const failed = r.results.filter((x) => !x.pass).length;
        const turned = r.results.some((x) => x.reoriented)
          ? " (turned to the way up that passed)"
          : "";
        setStatus(
          failed === 0
            ? `${stem} saved; every check passed${turned}`
            : `${stem} saved; ${String(failed)} failed`,
        );
      } else setStatus(`could not save: ${r.error}`);
    } catch (cause) {
      setStatus(`could not save: ${String(cause)}`);
    } finally {
      setBusy(false);
    }
  };

  const step = (d: number) => setIndex((i) => Math.min(Math.max(i + d, 0), s.frames.length - 1));

  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.metaKey || e.ctrlKey || e.altKey) return;
      const t = s.targets[Number.parseInt(e.key, 10) - 1];
      if (t) setTargetId(t.id);
      else if (e.key === "j") step(1);
      else if (e.key === "k") step(-1);
      else if (e.key === "s" && !busy()) void save();
      else if (e.key === "r") {
        const l = labelOf(targetId());
        if (l) setCorners(rotate(l.corners));
      } else if (e.key === "x") clearTarget();
      else if (e.key === "z") setNatural((n) => !n);
      else if (e.key === "Escape") setPlacing([]);
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  const frameStatus = (stem: string) => {
    if (dirty[stem]) return "unsaved";
    const r = results[stem];
    if (!r) return (labels[stem]?.length ?? 0) > 0 ? "saved" : "";
    return r.every((x) => x.pass) ? "pass" : "fail";
  };

  return (
    <div class={styles.page}>
      <div class={styles.bar}>
        <span class={styles.title}>Chart labels — {s.roll}</span>
        <button class={css(styles.button)} onClick={() => step(-1)}>
          ‹ k
        </button>
        <span>
          {frame().stem}
          <Show when={frame().shot}>
            <span class={styles.dim}> ({frame().shot})</span>
          </Show>
        </span>
        <button class={css(styles.button)} onClick={() => step(1)}>
          j ›
        </button>
        <For each={s.targets}>
          {(t, i) => (
            <button
              class={css(styles.button, t.id === targetId() && styles.active)}
              onClick={() => setTargetId(t.id)}
            >
              {String(i() + 1)} {t.id}
              {labelOf(t.id) ? " ✓" : ""}
            </button>
          )}
        </For>
        <button
          class={css(styles.button)}
          onClick={() => {
            const l = labelOf(targetId());
            if (l) setCorners(rotate(l.corners));
          }}
        >
          r rotate
        </button>
        <button class={css(styles.button)} onClick={clearTarget}>
          x clear
        </button>
        <label class={styles.dim}>
          card{" "}
          <input
            class={styles.input}
            value={card()}
            onInput={(e) => setCard(e.currentTarget.value)}
          />
        </label>
        <button class={css(styles.button)} disabled={busy()} onClick={() => void save()}>
          s save &amp; check
        </button>
        <button class={css(styles.button)} onClick={() => setNatural((n) => !n)}>
          z {natural() ? "fit" : "1:1"}
        </button>
        <span class={styles.dim}>{status()}</span>
      </div>
      <div class={styles.body}>
        <div class={styles.side}>
          <For each={s.frames}>
            {(f, i) => (
              <button
                class={css(styles.frameButton, i() === index() && styles.frameActive)}
                onClick={() => setIndex(i())}
              >
                {f.stem}{" "}
                <span class={frameStatus(f.stem) === "fail" ? styles.bad : ""}>
                  {frameStatus(f.stem)}
                </span>
              </button>
            )}
          </For>
        </div>
        <div class={styles.stage}>
          <Show
            when={frame().image}
            fallback={<p>No render of {frame().stem} in the renders folder.</p>}
          >
            <div class={styles.holder}>
              <img
                src={frame().image}
                alt={frame().stem}
                class={natural() ? styles.imageNatural : styles.imageFit}
                draggable={false}
              />
              <svg
                ref={(el) => (svg = el)}
                class={styles.overlay}
                viewBox={`0 0 ${String(frame().width)} ${String(frame().height)}`}
                onPointerDown={(e) => place(e)}
                style={{ cursor: labelOf(targetId()) && !pending() ? "default" : "crosshair" }}
              >
                <For each={s.targets}>
                  {(t) => (
                    <Show when={labelOf(t.id)}>
                      {(l) => (
                        <TargetOverlay
                          target={t}
                          label={l()}
                          width={frame().width}
                          height={frame().height}
                          selected={t.id === targetId()}
                          failing={failingIds(
                            results[frame().stem]?.find((r) => r.target === t.id)?.failures ?? [],
                          )}
                          onCorner={(corner, e) => startDrag(t.id, corner, e)}
                        />
                      )}
                    </Show>
                  )}
                </For>
                <For each={placing()}>
                  {(p) => (
                    <circle
                      cx={p[0] * frame().width}
                      cy={p[1] * frame().height}
                      r={Math.max(frame().width, frame().height) / 200}
                      class={css({ fill: "patch.line" })}
                    />
                  )}
                </For>
              </svg>
            </div>
          </Show>
        </div>
        <div class={styles.panel}>
          <Show
            when={!labelOf(targetId())}
            fallback={
              <p class={styles.dim}>
                Drag a corner to adjust. The grid may show upside down until you save: the check
                reads the scan and turns it the way up that passes. <b>r</b> turns it a quarter turn
                by hand; <b>x</b> clears it.
              </p>
            }
          >
            <p class={styles.prompt}>
              Click the four outer corners of the {target().id} patch array, in any order (
              {String(placing().length + 1)} of 4).
            </p>
            <p class={styles.dim}>{target().corners}</p>
          </Show>
          <Show when={pending()}>
            {(p) => (
              <p class={styles.prompt}>
                Several ways up pass for {p().target.id}. Click {p().target.anchor.prompt} to say
                which.
              </p>
            )}
          </Show>
          <Show when={seededFrom[frame().stem]}>
            <p class={styles.dim}>Started from {seededFrom[frame().stem]}'s corners.</p>
          </Show>
          <For each={results[frame().stem] ?? []}>{(r) => <ResultView result={r} />}</For>
          <p class={styles.dim}>Labels file: {s.labelsFile}</p>
        </div>
      </div>
    </div>
  );
}

function ResultView(props: { result: CheckResult }) {
  const r = () => props.result;
  return (
    <div class={css({ marginBlock: "12px" })}>
      <div class={r().pass ? "" : styles.bad}>
        <b>{r().target}</b>: {r().pass ? "pass" : "FAIL"}
      </div>
      <div class={styles.dim}>
        worst spread {r().worst_spread.toFixed(2)} stops; worst gap{" "}
        {r().worst_gap === null ? "—" : r().worst_gap!.toFixed(2)}; neutral steps{" "}
        {r()
          .neutral_steps.map((x) => x.toFixed(2))
          .join(" ")}
      </div>
      <For each={r().failures}>{(f) => <div class={styles.bad}>{f}</div>}</For>
      <For each={r().warnings}>{(w) => <div class={styles.dim}>warning: {w}</div>}</For>
    </div>
  );
}

function TargetOverlay(props: {
  target: Target;
  label: ChartLabel;
  width: number;
  height: number;
  selected: boolean;
  failing: Set<string>;
  onCorner: (corner: number, e: PointerEvent) => void;
}) {
  const px = (p: Point) => `${String(p[0] * props.width)},${String(p[1] * props.height)}`;
  const poly = (q: Quad) => q.map(px).join(" ");
  const patches = createMemo(() => projectPatches(props.target, props.label.corners) ?? []);
  const r = () => Math.max(props.width, props.height) / 160;
  const lineWidth = () => (props.selected ? 2 : 1);
  return (
    <g class={css({ opacity: props.selected ? 1 : 0.6 })}>
      <polygon
        points={poly(props.label.corners)}
        fill="none"
        class={css({ stroke: "accent" })}
        stroke-width={lineWidth()}
        vector-effect="non-scaling-stroke"
      />
      <For each={patches()}>
        {(p) => (
          <>
            <polygon
              points={poly(p.quad)}
              fill="none"
              class={
                props.failing.has(p.id) ? css({ stroke: "bad" }) : css({ stroke: "patch.line" })
              }
              stroke-width={lineWidth()}
              stroke-dasharray={p.neutral ? "4 3" : undefined}
              vector-effect="non-scaling-stroke"
            />
            <polygon points={poly(p.sample)} class={css({ fill: "patch.wash" })} />
          </>
        )}
      </For>
      <For each={[0, 1, 2, 3] as const}>
        {(i) => (
          <circle
            cx={props.label.corners[i][0] * props.width}
            cy={props.label.corners[i][1] * props.height}
            r={i === 0 ? r() * 1.5 : r()}
            class={css({ fill: "patch.halo", stroke: "patch.line", cursor: "move" })}
            stroke-width={2}
            vector-effect="non-scaling-stroke"
            onPointerDown={(e) => props.onCorner(i, e)}
          />
        )}
      </For>
    </g>
  );
}
