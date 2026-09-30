import { env } from "cloudflare:test";
import { exports } from "cloudflare:workers";
import validRequests from "../../../contracts/telemetry/upload-v1/requests/valid.json";
import invalidRequests from "../../../contracts/telemetry/upload-v1/requests/invalid.json";
import validResponses from "../../../contracts/telemetry/upload-v1/responses/valid.json";
import invalidResponses from "../../../contracts/telemetry/upload-v1/responses/invalid.json";
import { handleEvents } from "../src/ingest";

export interface RequestCase {
  name: string;
  request: unknown;
  expect?: string;
}
export const corpus = {
  validRequests: validRequests as RequestCase[],
  invalidRequests: invalidRequests as RequestCase[],
  validResponses: validResponses as { name: string; response: unknown }[],
  invalidResponses: invalidResponses as { name: string; response: unknown }[],
};

export const DAY = 20_613; // the corpus's event_day
export const NOW = DAY * 86_400_000 + 12 * 3_600_000;

type Event = Record<string, unknown> & { event_id: string };

/** A deep copy of the corpus's full-success event, with its own id. */
export function event(id: number | string, patch: Record<string, unknown> = {}): Event {
  const base = (corpus.validRequests[0]!.request as { events: Event[] }).events[0]!;
  return { ...structuredClone(base), event_id: hexId(id), ...patch };
}

export function hexId(id: number | string): string {
  return typeof id === "number" ? id.toString(16).padStart(32, "0") : id;
}

export function body(events: unknown[]): { upload_schema_version: 1; events: unknown[] } {
  return { upload_schema_version: 1, events };
}

/** The Worker's own entry point, as deployed. */
export const self = (exports as unknown as { default: Fetcher }).default;

let ip = 0;
/** A request from its own address, so the rate limiter never sees a repeat. */
export function request(payload: unknown, init: { ip?: string; raw?: string; headers?: Record<string, string> } = {}) {
  return new Request("https://ingest.test/v1/events", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "cf-connecting-ip": init.ip ?? `10.0.${ip >> 8}.${ip++ & 255}`,
      ...init.headers,
    },
    body: init.raw ?? JSON.stringify(payload),
  });
}

/** Through the deployed entry point, at the real clock. */
export function post(payload: unknown, init?: Parameters<typeof request>[1]) {
  return self.fetch(request(payload, init));
}

/** Through the handler at a chosen clock and env, for ceilings and day rules. */
export function ingest(payload: unknown, opts: { now?: number; env?: Partial<Env>; ip?: string; raw?: string } = {}) {
  return handleEvents(request(payload, opts), { ...env, ...opts.env } as Env, opts.now ?? NOW);
}

export async function reset(): Promise<void> {
  await env.DB.batch(
    [
      "DELETE FROM events",
      "DELETE FROM quarantine",
      "DELETE FROM daily_usage",
      "DELETE FROM cohort_usage",
      "DELETE FROM allowed_releases",
      "INSERT INTO allowed_releases (nc_version) VALUES ('0.1.0')",
      "UPDATE control SET value = '1' WHERE key = 'ingest_enabled'",
    ].map((s) => env.DB.prepare(s)),
  );
}

export async function count(table: string): Promise<number> {
  return (await env.DB.prepare(`SELECT count(*) AS n FROM ${table}`).first<{ n: number }>())!.n;
}

export interface Reply {
  upload_schema_version: 1;
  accepted: string[];
  duplicate: string[];
  rejected: { event_id: string; code: string }[];
}
