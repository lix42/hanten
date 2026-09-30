// Asserts the deploy-time promises wrangler.jsonc makes: no request logging, no
// binding beyond D1 and the rate limiter, and cost ceilings within the cost model
// (README.md). Runs in `pnpm check` and before every deploy.
import { readFileSync } from "node:fs";
import { parse } from "jsonc-parser";

const config = parse(readFileSync(new URL("../wrangler.jsonc", import.meta.url), "utf8"));
const failures = [];
const expect = (ok, message) => ok || failures.push(message);

const ALLOWED_KEYS = new Set([
  "$schema",
  "name",
  "main",
  "compatibility_date",
  "workers_dev",
  "preview_urls",
  "observability",
  "logpush",
  "limits",
  "d1_databases",
  "ratelimits",
  "triggers",
  "vars",
]);
for (const key of Object.keys(config)) expect(ALLOWED_KEYS.has(key), `unexpected top-level key "${key}"`);

expect(config.name === "hanten-telemetry", "name must be hanten-telemetry");
expect(config.observability?.enabled === false, "observability.enabled must be false");
expect(config.observability?.logs?.enabled === false, "observability.logs.enabled must be false");
expect(config.observability?.logs?.invocation_logs === false, "observability.logs.invocation_logs must be false");
expect(config.logpush === false, "logpush must be false");
expect(config.preview_urls === false, "preview_urls must be false");
expect(config.limits?.cpu_ms > 0 && config.limits.cpu_ms <= 100, "limits.cpu_ms must be in (0, 100]");

const d1 = config.d1_databases ?? [];
expect(
  d1.length === 1 && d1[0].binding === "DB" && d1[0].database_name === "hanten-telemetry",
  "exactly one D1 binding, DB → hanten-telemetry",
);
expect(/^[0-9a-f-]{36}$/.test(d1[0]?.database_id ?? ""), "the D1 binding needs its database_id");

const rl = config.ratelimits ?? [];
expect(
  rl.length === 1 && rl[0].name === "RATE_LIMITER" && rl[0].simple?.period === 60,
  "exactly one RATE_LIMITER, per minute",
);
expect(rl[0]?.simple?.limit <= 60, "RATE_LIMITER limit must be at most 60 a minute");

expect(config.triggers?.crons?.length === 1, "exactly one cron (retention)");

// Ceilings: the cost model's worst case is computed at these maxima.
const MAXIMA = {
  MAX_EVENTS_PER_DAY: 20_000,
  MAX_BYTES_PER_DAY: 50_000_000,
  MAX_COHORT_EVENTS_PER_DAY: 20_000,
  MAX_STORED_EVENTS: 1_750_000,
  RETENTION_DELETE_ROWS_PER_RUN: 200_000,
};
const vars = config.vars ?? {};
for (const [name, max] of Object.entries(MAXIMA)) {
  const n = Number(vars[name]);
  expect(Number.isSafeInteger(n) && n > 0 && n <= max, `${name} must be a positive integer ≤ ${max}`);
}
expect(vars.RETENTION_DAYS === "180", "RETENTION_DAYS must be 180 (docs/telemetry-strategy.md)");
for (const name of Object.keys(vars)) expect(name in MAXIMA || name === "RETENTION_DAYS", `unexpected var ${name}`);

if (failures.length) {
  console.error(`wrangler.jsonc breaks the deploy contract:\n  - ${failures.join("\n  - ")}`);
  process.exit(1);
}
console.log("wrangler.jsonc: deploy contract holds");
