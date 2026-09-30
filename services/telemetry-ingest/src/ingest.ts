// POST /v1/events. Status codes: 200 with per-event results; 400 for a malformed
// request (never retried); 429 and 503 for retryable refusals (README).
import { MAX_BODY_BYTES, isValidEnvelope, judgeEvent, type RejectionCode, type UploadEvent } from "./contract";
import { dayOf, limits, MS_PER_DAY, type Limits } from "./config";

interface Rejection {
  event_id: string;
  code: RejectionCode;
}

interface State {
  enabled: boolean;
  allowed: Set<string>;
  blocked: Set<string>;
  usage: { events: number; bytes: number };
  cohorts: Map<string, number>;
  stored: number;
}

type Route = { event: UploadEvent; table: "events" } | { event: UploadEvent; table: "quarantine"; reason: string };

/** Never throws: anything unexpected is a retryable 503. */
export async function handleEvents(request: Request, env: Env, nowMs: number): Promise<Response> {
  try {
    return await ingest(request, env, nowMs);
  } catch {
    return refuse(503, "unavailable", 60);
  }
}

async function ingest(request: Request, env: Env, nowMs: number): Promise<Response> {
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

  const cfg = limits(env);
  const today = dayOf(nowMs);
  const untilTomorrow = Math.ceil(((today + 1) * MS_PER_DAY - nowMs) / 1000);
  const events = body.events as UploadEvent[];
  const state = await readState(env.DB, today, events);
  // Fast refusals; the CHECK constraints in the write batch are the real bounds.
  if (!state.enabled) return refuse(503, "ingestion_disabled", 3600);
  if (state.stored >= cfg.maxStoredEvents) return refuse(503, "storage_full", 3600);
  if (
    state.usage.events + events.length > cfg.maxEventsPerDay ||
    state.usage.bytes + bytes.byteLength > cfg.maxBytesPerDay
  ) {
    return refuse(429, "daily_limit", untilTomorrow);
  }

  const rejected: Rejection[] = [];
  const routes: Route[] = [];
  const cohortAdds = new Map<string, number>();
  for (const event of events) {
    const code = judgeEvent(event) ?? (state.blocked.has(event.nc_version) ? "release_blocked" : null);
    if (code) rejected.push({ event_id: event.event_id, code });
    else routes.push(route(event, today, cfg, state, cohortAdds));
  }

  let results: D1Result[];
  try {
    results = await env.DB.batch([
      env.DB.prepare(`UPDATE storage SET max_events = ?1 WHERE id = 1 AND max_events != ?1`).bind(cfg.maxStoredEvents),
      env.DB.prepare(
        `INSERT INTO daily_usage (day, requests, events, bytes, max_events, max_bytes)
         VALUES (?1, 1, ?2, ?3, ?4, ?5)
         ON CONFLICT (day) DO UPDATE SET requests = requests + 1,
           events = events + excluded.events, bytes = bytes + excluded.bytes,
           max_events = excluded.max_events, max_bytes = excluded.max_bytes`,
      ).bind(today, events.length, bytes.byteLength, cfg.maxEventsPerDay, cfg.maxBytesPerDay),
      ...routes.map((r) => insert(env.DB, r, today)),
    ]);
  } catch (e) {
    const message = String(e instanceof Error ? `${e.message} ${String(e.cause ?? "")}` : e);
    if (message.includes("daily_limit")) return refuse(429, "daily_limit", untilTomorrow);
    if (message.includes("storage_limit")) return refuse(503, "storage_full", 3600);
    throw e;
  }

  const inserted = new Set<string>();
  for (const r of results.slice(2)) {
    for (const row of r.results as { event_id: string }[]) inserted.add(row.event_id);
  }
  const accepted: string[] = [];
  const duplicate: string[] = [];
  for (const { event } of routes) (inserted.has(event.event_id) ? accepted : duplicate).push(event.event_id);
  return Response.json({ upload_schema_version: 1, accepted, duplicate, rejected });
}

/**
 * Where an accepted event goes. An anomalous day, a release not yet allowlisted,
 * or a cohort past its daily volume is kept out of the analytical table; the
 * client still sees the event accepted.
 */
function route(event: UploadEvent, today: number, cfg: Limits, state: State, adds: Map<string, number>): Route {
  if (event.event_day > today + 1 || event.event_day < today - cfg.retentionDays) {
    return { event, table: "quarantine", reason: "event_day" };
  }
  if (!state.allowed.has(event.nc_version)) return { event, table: "quarantine", reason: "release" };
  const cohort = cohortKey(event.nc_version, event.platform.os, event.platform.arch);
  const count = (state.cohorts.get(cohort) ?? 0) + (adds.get(cohort) ?? 0);
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
        `INSERT INTO quarantine (event_id, received_day, reason, nc_version, payload)
         SELECT ?1, ?2, ?3, ?4, ?5 WHERE NOT EXISTS (SELECT 1 FROM events WHERE event_id = ?1)
         ON CONFLICT (event_id) DO NOTHING RETURNING event_id`,
      )
      .bind(e.event_id, today, r.reason, e.nc_version, payload);
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

/** One read batch, limited to the releases the request names. */
async function readState(db: D1Database, today: number, events: UploadEvent[]): Promise<State> {
  // Unvalidated events may lack a string nc_version; they never reach a lookup.
  const versions = JSON.stringify([...new Set(events.map((e) => e.nc_version).filter((v) => typeof v === "string"))]);
  const [control, allowed, blocked, usage, cohorts, storage] = await db.batch([
    db.prepare(`SELECT value FROM control WHERE key = 'ingest_enabled'`),
    db
      .prepare(`SELECT nc_version FROM allowed_releases WHERE nc_version IN (SELECT value FROM json_each(?1))`)
      .bind(versions),
    db
      .prepare(`SELECT nc_version FROM blocked_releases WHERE nc_version IN (SELECT value FROM json_each(?1))`)
      .bind(versions),
    db.prepare(`SELECT events, bytes FROM daily_usage WHERE day = ?1`).bind(today),
    db
      .prepare(
        `SELECT nc_version, os, arch, events FROM cohort_usage
         WHERE day = ?1 AND nc_version IN (SELECT value FROM json_each(?2))`,
      )
      .bind(today, versions),
    db.prepare(`SELECT events FROM storage WHERE id = 1`),
  ]);
  const rows = <T>(r: D1Result | undefined) => (r?.results ?? []) as T[];
  const u = rows<{ events: number; bytes: number }>(usage)[0];
  return {
    enabled: rows<{ value: string }>(control)[0]?.value === "1",
    allowed: new Set(rows<{ nc_version: string }>(allowed).map((r) => r.nc_version)),
    blocked: new Set(rows<{ nc_version: string }>(blocked).map((r) => r.nc_version)),
    usage: { events: u?.events ?? 0, bytes: u?.bytes ?? 0 },
    cohorts: new Map(
      rows<{ nc_version: string; os: string; arch: string; events: number }>(cohorts).map((r) => [
        cohortKey(r.nc_version, r.os, r.arch),
        r.events,
      ]),
    ),
    stored: rows<{ events: number }>(storage)[0]!.events,
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
