import { createServerFn } from "@tanstack/solid-start";
import { statSync } from "node:fs";
import { labelProblems, type ChartLabel, type CheckResult, type Target } from "../chart";
import { bootId } from "./boot";
import {
  labelConfig,
  loadFrames,
  loadTargets,
  readLabels,
  runCheck,
  withFrame,
  writeLabels,
} from "./labelSession";

export interface LabelFramePayload {
  readonly stem: string;
  readonly width: number;
  readonly height: number;
  readonly shot: string | undefined;
  /** Where the page loads the render, or `undefined` when there is none. */
  readonly image: string | undefined;
}

export type LabelSessionPayload =
  | {
      readonly ok: true;
      readonly roll: string;
      readonly card: string;
      readonly labelsFile: string;
      readonly frames: readonly LabelFramePayload[];
      readonly targets: readonly Target[];
      readonly labels: Readonly<Record<string, readonly ChartLabel[]>>;
      readonly boot: string;
    }
  | { readonly ok: false; readonly error: string };

/** Everything the labelling page needs, read fresh on each load. */
export const getLabelSession = createServerFn({ method: "GET" }).handler(
  async (): Promise<LabelSessionPayload> => {
    const config = labelConfig(process.env);
    if (typeof config === "string") return { ok: false, error: config };
    const frames = loadFrames(config);
    if (typeof frames === "string") return { ok: false, error: frames };
    try {
      return {
        ok: true,
        roll: config.roll,
        card: config.card,
        labelsFile: config.labelsFile,
        frames: frames.map((f) => ({
          stem: f.stem,
          width: f.width,
          height: f.height,
          shot: f.shot,
          image:
            f.render === undefined
              ? undefined
              : `/label-img/${encodeURIComponent(f.stem)}?v=${String(statSync(f.render).mtimeMs)}`,
        })),
        targets: loadTargets(config.repo),
        labels: readLabels(config).frames,
        boot: bootId(),
      };
    } catch (cause) {
      return { ok: false, error: cause instanceof Error ? cause.message : String(cause) };
    }
  },
);

export interface SaveRequest {
  readonly stem: string;
  readonly labels: readonly ChartLabel[];
}

export type SaveResult =
  | { readonly ok: true; readonly results: readonly CheckResult[] }
  | { readonly ok: false; readonly error: string };

function isSaveRequest(data: unknown): data is SaveRequest {
  const d = data as SaveRequest;
  return (
    typeof d === "object" &&
    d !== null &&
    typeof d.stem === "string" &&
    Array.isArray(d.labels) &&
    d.labels.every(
      (l) =>
        typeof l.target === "string" &&
        typeof l.card === "string" &&
        Array.isArray(l.corners) &&
        l.corners.length === 4 &&
        l.corners.every(
          (p: unknown) => Array.isArray(p) && p.length === 2 && p.every(Number.isFinite),
        ),
    )
  );
}

/** Write one frame's labels to the labels file, then check them against the scan. */
export const saveLabels = createServerFn({ method: "POST" })
  .validator((data: unknown): SaveRequest => {
    if (!isSaveRequest(data)) throw new Error("not a frame's labels");
    return data;
  })
  .handler(async ({ data }): Promise<SaveResult> => {
    const config = labelConfig(process.env);
    if (typeof config === "string") return { ok: false, error: config };
    // Refused before writing: nctool validates the whole file, so one bad label saved
    // would break the check and import of every frame.
    const problems = data.labels.flatMap((l) => labelProblems(l).map((p) => `${l.target}: ${p}`));
    if (problems.length > 0) return { ok: false, error: problems.join("; ") };
    try {
      writeLabels(config, withFrame(readLabels(config), data.stem, data.labels));
      const results = data.labels.length > 0 ? await runCheck(config, data.stem) : [];
      // The check fixes which corner is the target's top left; keep its order.
      if (results.some((r) => r.reoriented)) {
        const fixed = data.labels.map((l, i) => ({
          ...l,
          corners: results[i]?.corners ?? l.corners,
        }));
        writeLabels(config, withFrame(readLabels(config), data.stem, fixed));
      }
      return { ok: true, results };
    } catch (cause) {
      return { ok: false, error: cause instanceof Error ? cause.message : String(cause) };
    }
  });
