// POST /v1/events. Status codes: 200 with per-event results; 400 for a malformed
// request (never retried); 429 and 503 for retryable refusals — rate limit, a
// daily ceiling, the kill switch, full storage or a D1 failure.
import { MAX_BODY_BYTES, isValidEnvelope, judgeEvent, type RejectionCode, type UploadEvent } from "./contract";
import { dayOf, limits, MS_PER_DAY, type Limits } from "./config";

interface Rejection {
  event_id: string;
  code: RejectionCode;
}

interface State {
  enabled: boolean;
  releases: Set<string>;
  usage: { events: number; bytes: number };
  cohorts: Map<string, number>;
  dbBytes: number;
}

type Route = { event: UploadEvent; table: "events" } | { event: UploadEvent; table: "quarantine"; reason: string };

export async function handleEvents(request: Request, env: Env, nowMs: number): Promise<Response> {
  const key = request.headers.get("cf-connecting-ip") ?? "unknown";
  if (!(await env.RATE_LIMITER.limit({ key })).success) return refuse(429, "rate_limited", 60);

  const bytes = await readCapped(request);
  if (bytes === null) return refuse(400, "body_too_large");
  let body: unknown;
  try {
    body = JSON.parse(new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(bytes));
  } catch {
    return refuse(400, "malformed_json");
  }
  if (!isValidEnvelope(body)) return refuse(400, "invalid_envelope");

  let cfg: Limits;
  let state: State;
  const today = dayOf(nowMs);
  try {
    cfg = limits(env);
    state = await readState(env.DB, today);
  } catch {
    return refuse(503, "unavailable", 60);
  }
  const untilTomorrow = Math.ceil(((today + 1) * MS_PER_DAY - nowMs) / 1000);
  if (!state.enabled) return refuse(503, "ingestion_disabled", 3600);
  if (state.dbBytes > cfg.maxDbBytes) return refuse(503, "storage_full", 3600);
  if (state.usage.events + body.events.length > cfg.maxEventsPerDay) {
    return refuse(429, "daily_event_limit", untilTomorrow);
  }
  if (state.usage.bytes + bytes.byteLength > cfg.maxBytesPerDay) {
    return refuse(429, "daily_byte_limit", untilTomorrow);
  }

  const rejected: Rejection[] = [];
  const routes: Route[] = [];
  const cohortAdds = new Map<string, number>();
  for (const raw of body.events) {
    const event = raw as UploadEvent;
    const code = judgeEvent(event) ?? (state.releases.has(event.nc_version) ? null : "release_blocked");
    if (code) {
      rejected.push({ event_id: event.event_id, code });
      continue;
    }
    routes.push(route(event, today, cfg, state.cohorts, cohortAdds));
  }

  const inserted = new Set<string>();
  try {
    const results = await env.DB.batch([
      ...routes.map((r) => insert(env.DB, r, today)),
      env.DB.prepare(
        `INSERT INTO daily_usage (day, requests, events, bytes) VALUES (?1, 1, ?2, ?3)
         ON CONFLICT (day) DO UPDATE SET requests = requests + 1,
           events = events + excluded.events, bytes = bytes + excluded.bytes`,
      ).bind(today, body.events.length, bytes.byteLength),
      ...[...cohortAdds].map(([cohort, n]) => {
        const [nc_version, os, arch] = cohort.split("\0");
        return env.DB.prepare(
          `INSERT INTO cohort_usage (day, nc_version, os, arch, events) VALUES (?1, ?2, ?3, ?4, ?5)
           ON CONFLICT (day, nc_version, os, arch) DO UPDATE SET events = events + excluded.events`,
        ).bind(today, nc_version, os, arch, n);
      }),
    ]);
    for (const r of results.slice(0, routes.length)) {
      for (const row of r.results as { event_id: string }[]) inserted.add(row.event_id);
    }
  } catch {
    return refuse(503, "unavailable", 60);
  }

  const accepted: string[] = [];
  const duplicate: string[] = [];
  for (const { event } of routes) (inserted.has(event.event_id) ? accepted : duplicate).push(event.event_id);
  return Response.json({ upload_schema_version: 1, accepted, duplicate, rejected });
}

/**
 * Where an accepted event goes. Anomalous days and a cohort over its daily volume
 * are kept out of the analytical table; the client still sees them accepted.
 */
function route(
  event: UploadEvent,
  today: number,
  cfg: Limits,
  stored: Map<string, number>,
  adds: Map<string, number>,
): Route {
  if (event.event_day > today + 1 || event.event_day < today - cfg.retentionDays) {
    return { event, table: "quarantine", reason: "event_day" };
  }
  const cohort = cohortKey(event.nc_version, event.platform.os, event.platform.arch);
  const count = (stored.get(cohort) ?? 0) + (adds.get(cohort) ?? 0);
  if (count >= cfg.maxCohortEventsPerDay) return { event, table: "quarantine", reason: "cohort_volume" };
  adds.set(cohort, (adds.get(cohort) ?? 0) + 1);
  return { event, table: "events" };
}

function cohortKey(ncVersion: string, os: string, arch: string): string {
  return `${ncVersion}\0${os}\0${arch}`;
}

/** An insert that returns the id only when the event is new to both tables. */
function insert(db: D1Database, r: Route, today: number): D1PreparedStatement {
  const e = r.event;
  const payload = JSON.stringify(e);
  if (r.table === "quarantine") {
    return db
      .prepare(
        `INSERT INTO quarantine (event_id, received_day, reason, payload)
         SELECT ?1, ?2, ?3, ?4 WHERE NOT EXISTS (SELECT 1 FROM events WHERE event_id = ?1)
         ON CONFLICT (event_id) DO NOTHING RETURNING event_id`,
      )
      .bind(e.event_id, today, r.reason, payload);
  }
  return db
    .prepare(
      `INSERT INTO events (event_id, received_day, event_day, source_schema_version, event_name,
         nc_version, os, arch, cpu_bucket, stage, status, error_kind, exit_code, encoding,
         megapixels_tenths, input_size_bucket, total_ms, payload)
       SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
       WHERE NOT EXISTS (SELECT 1 FROM quarantine WHERE event_id = ?1)
       ON CONFLICT (event_id) DO NOTHING RETURNING event_id`,
    )
    .bind(
      e.event_id,
      today,
      e.event_day,
      e.source_schema_version,
      e.event_name,
      e.nc_version,
      e.platform.os,
      e.platform.arch,
      e.platform.cpu_bucket,
      e.stage,
      e.outcome?.status ?? null,
      e.outcome?.error_kind ?? null,
      e.outcome?.exit_code ?? null,
      e.conversion?.encoding ?? null,
      e.image?.megapixels_tenths ?? null,
      e.image?.input_size_bucket ?? null,
      e.timing_ms?.total ?? null,
      payload,
    );
}

async function readState(db: D1Database, today: number): Promise<State> {
  const [control, releases, usage, cohorts] = await db.batch([
    db.prepare(`SELECT value FROM control WHERE key = 'ingest_enabled'`),
    db.prepare(`SELECT nc_version FROM allowed_releases`),
    db.prepare(`SELECT events, bytes FROM daily_usage WHERE day = ?1`).bind(today),
    db.prepare(`SELECT nc_version, os, arch, events FROM cohort_usage WHERE day = ?1`).bind(today),
  ]);
  const u = (usage!.results as { events: number; bytes: number }[])[0];
  return {
    enabled: (control!.results as { value: string }[])[0]?.value === "1",
    releases: new Set((releases!.results as { nc_version: string }[]).map((r) => r.nc_version)),
    usage: { events: u?.events ?? 0, bytes: u?.bytes ?? 0 },
    cohorts: new Map(
      (cohorts!.results as { nc_version: string; os: string; arch: string; events: number }[]).map((r) => [
        cohortKey(r.nc_version, r.os, r.arch),
        r.events,
      ]),
    ),
    dbBytes: control!.meta.size_after,
  };
}

/** The body, or `null` once it passes the cap; stops reading there. */
async function readCapped(request: Request): Promise<Uint8Array | null> {
  const declared = Number(request.headers.get("content-length"));
  if (declared > MAX_BODY_BYTES) return null;
  if (!request.body) return new Uint8Array();
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > MAX_BODY_BYTES) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const out = new Uint8Array(total);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.byteLength;
  }
  return out;
}

function refuse(status: number, error: string, retryAfterSeconds?: number): Response {
  const headers: Record<string, string> = {};
  if (retryAfterSeconds !== undefined) headers["retry-after"] = String(retryAfterSeconds);
  return Response.json({ error }, { status, headers });
}
