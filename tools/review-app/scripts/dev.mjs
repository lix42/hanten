#!/usr/bin/env node
/**
 * `pnpm dev [path to review.json | its directory] [vp dev flags...]`
 *
 * Sugar over the real contract, which is the `REVIEW_SET` environment variable:
 * the server reads that once at startup. This exists because `vp dev` has no way
 * to forward an argument of its own, and typing a path is what you actually want
 * to do. An explicit `REVIEW_SET` in the environment wins if no path is given.
 */

import { spawn } from "node:child_process";
import { resolve } from "node:path";

const [, , ...argv] = process.argv;
// Anything starting with `-` belongs to `vp dev`; a bare first word is the set.
const stated = argv[0]?.startsWith("-") ? undefined : argv[0];
const passthrough = stated === undefined ? argv : argv.slice(1);

const env = { ...process.env };
if (stated !== undefined) env["REVIEW_SET"] = resolve(stated);

// On Windows the local `vp` is a `vp.cmd` shim, which `spawn` cannot execute
// directly without a shell. The set path travels in the environment rather than
// in argv, so nothing user-supplied is handed to that shell.
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
