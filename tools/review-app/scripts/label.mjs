#!/usr/bin/env node
/**
 * `pnpm label --roll <roll> --renders <dir> --labels <file> [--frames a,b] [--card id]
 *   [--asset-root <dir>] [vp dev flags...]`
 *
 * Starts the app for chart labelling and prints the page to open (`/label`). Like
 * `pnpm dev`, everything travels in the environment (`LABEL_*`), which the server
 * reads; see `src/server/labelSession.ts`. The asset root defaults to
 * `$NC_ASSET_ROOT`, else the repository's `../nc-assets`.
 */

import { spawn } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const flags = {
  "--roll": "LABEL_ROLL",
  "--renders": "LABEL_RENDERS",
  "--labels": "LABEL_FILE",
  "--frames": "LABEL_FRAMES",
  "--card": "LABEL_CARD",
  "--asset-root": "LABEL_ASSET_ROOT",
};
const paths = new Set(["LABEL_RENDERS", "LABEL_FILE", "LABEL_ASSET_ROOT"]);

const env = { ...process.env, LABEL_REPO: repo };
env["LABEL_ASSET_ROOT"] ??= process.env["NC_ASSET_ROOT"] ?? resolve(repo, "../nc-assets");
const passthrough = [];
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  const key = flags[argv[i]];
  if (key === undefined) {
    passthrough.push(argv[i]);
    continue;
  }
  const value = argv[++i];
  if (value === undefined) {
    console.error(`${argv[i - 1]} needs a value`);
    process.exit(2);
  }
  env[key] = paths.has(key) ? resolve(value) : value;
}
for (const [flag, key] of Object.entries(flags).slice(0, 3)) {
  if (!env[key]) {
    console.error(`${flag} is required (pnpm label --roll R --renders DIR --labels FILE)`);
    process.exit(2);
  }
}
if (env["LABEL_ASSET_ROOT"]) env["LABEL_ASSET_ROOT"] = resolve(env["LABEL_ASSET_ROOT"]);
console.log("chart labelling: open /label on the address below");

const child = spawn("vp", ["dev", ...passthrough], {
  stdio: "inherit",
  env,
  shell: process.platform === "win32",
});
child.on("error", (cause) => {
  console.error(`could not start \`vp dev\`: ${cause.message}`);
  process.exit(1);
});
child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code ?? 0);
});
