// Post-deploy smoke test: exercises validation, D1 and the allowlist without
// storing an event (a release no allowlist names is rejected before any insert).
// Usage: node scripts/smoke.mjs https://hanten-telemetry.<subdomain>.workers.dev
import { readFileSync } from "node:fs";

const base = process.argv[2]?.replace(/\/$/, "");
if (!base) throw new Error("usage: smoke.mjs <worker base URL>");
const url = `${base}/v1/events`;
const valid = JSON.parse(
  readFileSync(new URL("../../../contracts/telemetry/upload-v1/requests/valid.json", import.meta.url)),
);
const event = {
  ...valid[0].request.events[0],
  nc_version: "0.0.0-smoke",
  event_id: "5e0ce5e0ce5e0ce5e0ce5e0ce5e0ce5e",
};

const post = (body) =>
  fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
const checks = [];
const check = (name, ok, detail) => {
  checks.push(ok);
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok ? "" : `: ${detail}`}`);
};

const blocked = await post({ upload_schema_version: 1, events: [event] });
const reply = await blocked.json().catch(() => null);
check(
  "unlisted release is rejected, nothing stored",
  blocked.status === 200 && reply?.rejected?.[0]?.code === "release_blocked" && reply.accepted.length === 0,
  `${blocked.status} ${JSON.stringify(reply)}`,
);
const malformed = await post({ upload_schema_version: 2, events: [] });
check("malformed envelope is 400", malformed.status === 400, malformed.status);
const get = await fetch(url);
check("GET is 405", get.status === 405, get.status);

if (checks.includes(false)) process.exit(1);
