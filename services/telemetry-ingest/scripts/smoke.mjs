// Post-deploy smoke test. It stores nothing: it posts a release the migration
// puts in blocked_releases, which is rejected before any insert. A new Worker
// takes seconds to reach every edge, and until then Cloudflare answers for it
// (a bare 404 or 500), so the checks wait for the Worker's own answers first.
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
const answer = async (res) => `${res.status} ${(await res.text()).slice(0, 300)}`;

// The Worker's own 405 names the error in JSON; Cloudflare's placeholder does not.
const isWorker = async () => {
  const res = await fetch(url).catch(() => null);
  return res?.status === 405 && (await res.json().catch(() => null))?.error === "method_not_allowed";
};
const PROPAGATION_MS = 90_000;
let streak = 0;
for (const start = Date.now(); streak < 3;) {
  streak = (await isWorker()) ? streak + 1 : 0;
  if (Date.now() - start > PROPAGATION_MS) {
    console.log(`FAIL the Worker never answered three times in a row within ${PROPAGATION_MS / 1000} s`);
    process.exit(1);
  }
  await new Promise((r) => setTimeout(r, streak ? 500 : 3_000));
}
console.log("ok   the Worker answers (GET is its own 405)");

const res = await post({ upload_schema_version: 1, events: [event] });
const text = await res.text();
let reply = null;
try {
  reply = JSON.parse(text);
} catch {}
if (res.status === 503 && reply?.error === "ingestion_disabled") {
  console.log("note ingestion is disabled (kill switch); the D1 path was not exercised");
} else {
  check(
    "a blocked release is rejected, nothing stored",
    res.status === 200 && reply?.rejected?.[0]?.code === "release_blocked" && reply.accepted.length === 0,
    `${res.status} ${text.slice(0, 300)}`,
  );
}
const malformed = await post({ upload_schema_version: 2, events: [] });
check("a malformed envelope is 400", malformed.status === 400, await answer(malformed));

if (failed) process.exit(1);
