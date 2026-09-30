// Post-deploy smoke test. It stores nothing: it posts a release the migration
// puts in blocked_releases, which is rejected before any insert.
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
let failed = false;
const check = (name, ok, detail) => {
  failed ||= !ok;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok ? "" : `: ${detail}`}`);
};

const res = await post({ upload_schema_version: 1, events: [event] });
const reply = await res.json().catch(() => null);
if (res.status === 503 && reply?.error === "ingestion_disabled") {
  console.log("note ingestion is disabled (kill switch); the D1 path was not exercised");
} else {
  check(
    "a blocked release is rejected, nothing stored",
    res.status === 200 && reply?.rejected?.[0]?.code === "release_blocked" && reply.accepted.length === 0,
    `${res.status} ${JSON.stringify(reply)}`,
  );
}
const malformed = await post({ upload_schema_version: 2, events: [] });
check("a malformed envelope is 400", malformed.status === 400, malformed.status);
const get = await fetch(url);
check("GET is 405", get.status === 405, get.status);

if (failed) process.exit(1);
