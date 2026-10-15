/**
 * The chart-labelling session: which frames, which targets, and the labels file.
 *
 * Started by `pnpm label` (`scripts/label.mjs`), which states everything in the
 * environment: `LABEL_ROLL`, `LABEL_RENDERS` (a directory of renders named by frame
 * stem), `LABEL_FILE` (the labels file, created if absent), `LABEL_ASSET_ROOT`, and
 * `LABEL_REPO` (where `nctool` and the target definitions live). The page labels a
 * **render** for the eye; the check (`nctool manifest charts check`) reads the
 * **scan**, which the render must match pixel for pixel so fractions carry over.
 *
 * The labels file is the hand-off: `nctool manifest charts import` reads it, checks
 * every label again and writes the manifest. Nothing here writes the manifest.
 */

import { spawn } from "node:child_process";
import { existsSync, readFileSync, readdirSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { ChartLabel, CheckResult, Target } from "../chart";

export interface LabelConfig {
  readonly roll: string;
  readonly renders: string;
  readonly labelsFile: string;
  readonly assetRoot: string;
  readonly repo: string;
  readonly card: string;
  /** Frame stems to label; empty means every `chart` frame of the roll. */
  readonly frames: readonly string[];
  readonly python: string;
}

export interface LabelFrame {
  readonly stem: string;
  readonly width: number;
  readonly height: number;
  /** Absolute path of the render, or `undefined` when none was found. */
  readonly render: string | undefined;
  readonly shot: string | undefined;
}

export interface LabelsFile {
  readonly roll: string;
  readonly frames: Readonly<Record<string, readonly ChartLabel[]>>;
}

const RENDER_EXTENSIONS = [".jpg", ".jpeg", ".png", ".tif", ".tiff"];

/** The session's configuration, or a message naming what is missing. */
export function labelConfig(env: NodeJS.ProcessEnv): LabelConfig | string {
  const need = ["LABEL_ROLL", "LABEL_RENDERS", "LABEL_FILE", "LABEL_ASSET_ROOT", "LABEL_REPO"];
  const missing = need.filter((k) => !env[k]);
  if (missing.length > 0) {
    return `not started by \`pnpm label\` (missing ${missing.join(", ")}); see the README`;
  }
  const repo = env["LABEL_REPO"]!;
  return {
    roll: env["LABEL_ROLL"]!,
    renders: env["LABEL_RENDERS"]!,
    labelsFile: env["LABEL_FILE"]!,
    assetRoot: env["LABEL_ASSET_ROOT"]!,
    repo,
    card: env["LABEL_CARD"] || "ccpp2",
    frames: (env["LABEL_FRAMES"] ?? "").split(",").filter((s) => s !== ""),
    python:
      env["LABEL_PYTHON"] ||
      (existsSync(join(repo, ".venv/bin/python")) ? join(repo, ".venv/bin/python") : "python3"),
  };
}

interface ManifestFrame {
  readonly file: string;
  readonly role?: string;
  readonly width?: number;
  readonly height?: number;
  readonly shot?: { readonly exposure_ev?: number; readonly aperture?: number };
}

const stemOf = (file: string) => (file.split("/").pop() ?? file).replace(/\.[^.]+$/, "");

/** The frames to label, from the manifest's roll: the named stems, else its `chart` frames. */
export function selectFrames(
  manifest: unknown,
  roll: string,
  stems: readonly string[],
): readonly ManifestFrame[] | string {
  const rolls = (manifest as { rolls?: Record<string, { frames?: ManifestFrame[] }> }).rolls;
  const frames = rolls?.[roll]?.frames;
  if (!frames) return `the manifest has no roll ${JSON.stringify(roll)}`;
  if (stems.length === 0) {
    const charts = frames.filter((f) => f.role === "chart");
    return charts.length > 0
      ? charts
      : `roll ${roll} has no \`chart\` frames; name them with --frames`;
  }
  const out: ManifestFrame[] = [];
  for (const stem of stems) {
    const f = frames.find((x) => stemOf(x.file) === stem);
    if (!f) return `roll ${roll} has no frame ${JSON.stringify(stem)}`;
    out.push(f);
  }
  return out;
}

function shotText(shot: ManifestFrame["shot"]): string | undefined {
  if (!shot) return undefined;
  const parts = [];
  if (shot.exposure_ev !== undefined)
    parts.push(`${shot.exposure_ev >= 0 ? "+" : ""}${String(shot.exposure_ev)} EV`);
  if (shot.aperture !== undefined) parts.push(`f/${String(shot.aperture)}`);
  return parts.length > 0 ? parts.join(", ") : undefined;
}

export function findRender(
  renders: string,
  stem: string,
  listing: readonly string[],
): string | undefined {
  for (const ext of RENDER_EXTENSIONS) {
    const name = listing.find((f) => f.toLowerCase() === `${stem}${ext}`.toLowerCase());
    if (name) return join(renders, name);
  }
  return undefined;
}

export function loadFrames(config: LabelConfig): readonly LabelFrame[] | string {
  let manifest: unknown;
  try {
    manifest = JSON.parse(readFileSync(join(config.assetRoot, "manifest.json"), "utf8"));
  } catch (cause) {
    return `cannot read ${join(config.assetRoot, "manifest.json")}: ${String(cause)}`;
  }
  const frames = selectFrames(manifest, config.roll, config.frames);
  if (typeof frames === "string") return frames;
  let listing: string[];
  try {
    listing = readdirSync(config.renders);
  } catch (cause) {
    return `cannot list the renders in ${config.renders}: ${String(cause)}`;
  }
  return frames.map((f) => ({
    stem: stemOf(f.file),
    width: f.width ?? 0,
    height: f.height ?? 0,
    render: findRender(config.renders, stemOf(f.file), listing),
    shot: shotText(f.shot),
  }));
}

export function loadTargets(repo: string): readonly Target[] {
  const dir = join(repo, "scripts/analysis/nctool/targets");
  return readdirSync(dir)
    .filter((f) => f.endsWith(".json"))
    .sort()
    .map((f) => JSON.parse(readFileSync(join(dir, f), "utf8")) as Target);
}

export function readLabels(config: LabelConfig): LabelsFile {
  if (!existsSync(config.labelsFile)) return { roll: config.roll, frames: {} };
  const data = JSON.parse(readFileSync(config.labelsFile, "utf8")) as LabelsFile;
  if (data.roll !== config.roll) {
    throw new Error(`${config.labelsFile} labels roll ${data.roll}, not ${config.roll}`);
  }
  return data;
}

/**
 * The labels file with one frame's labels replaced. A cleared frame stays, with no
 * labels, so `charts import` removes what it had imported for it before.
 */
export function withFrame(
  file: LabelsFile,
  stem: string,
  labels: readonly ChartLabel[],
): LabelsFile {
  const frames = { ...file.frames, [stem]: labels };
  const sorted = Object.fromEntries(
    Object.keys(frames)
      .sort()
      .map((k) => [k, frames[k]!]),
  );
  return { roll: file.roll, frames: sorted };
}

export function writeLabels(config: LabelConfig, file: LabelsFile): void {
  // Through a temporary file, so a crash mid-write never leaves half a file that
  // the next start refuses.
  const tmp = `${config.labelsFile}.tmp`;
  writeFileSync(tmp, `${JSON.stringify(file, null, 2)}\n`);
  renameSync(tmp, config.labelsFile);
}

/** `nctool manifest charts check` on one frame, read back as results per label. */
export function runCheck(config: LabelConfig, stem: string): Promise<readonly CheckResult[]> {
  const args = [
    "-m",
    "nctool",
    "manifest",
    "charts",
    "check",
    config.labelsFile,
    "--frame",
    stem,
    "--asset-root",
    config.assetRoot,
  ];
  return new Promise((resolve, reject) => {
    const child = spawn(config.python, args, {
      env: { ...process.env, PYTHONPATH: join(config.repo, "scripts/analysis") },
    });
    let out = "";
    let err = "";
    child.stdout.on("data", (d: Buffer) => (out += d.toString()));
    child.stderr.on("data", (d: Buffer) => (err += d.toString()));
    child.on("error", reject);
    child.on("close", (code) => {
      // 0 is every label passing, 1 at least one failing: both carry results.
      if (code !== 0 && code !== 1) {
        reject(new Error(err.trim() || `nctool exited ${String(code)}`));
        return;
      }
      try {
        const parsed = JSON.parse(out) as { frames: Record<string, CheckResult[]> };
        resolve(parsed.frames[stem] ?? []);
      } catch {
        // An uncaught exception also exits 1, with no JSON: say what nctool said.
        reject(new Error(err.trim() || "nctool printed no result"));
      }
    });
  });
}
