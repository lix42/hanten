import { createScheduledController, env } from "cloudflare:test";
import { beforeEach, expect, it } from "vitest";
import worker from "../src/index";
import { expire } from "../src/retention";
import { body, count, DAY, event, hexId, ingest, NOW, reset } from "./helpers";

const DAY_MS = 86_400_000;

beforeEach(reset);

/** Ingest one event per entry, received on that day. */
async function seed(days: number[], patch: Record<string, unknown> = {}) {
  let id = 1;
  for (const d of days) {
    const r = await ingest(body([event(id++, { event_day: d, ...patch })]), { now: d * DAY_MS });
    expect(r.status).toBe(200);
  }
}

it("deletes rows received more than 180 days ago and keeps the rest", async () => {
  await seed([DAY - 181, DAY - 180, DAY]);
  const { deleted } = await expire(env, NOW);
  expect(deleted).toBe(1);
  const ids = await env.DB.prepare("SELECT event_id FROM events ORDER BY event_id").all();
  expect(ids.results).toEqual([{ event_id: hexId(2) }, { event_id: hexId(3) }]);
  expect(await count("daily_usage")).toBe(2);
  expect(await count("cohort_usage")).toBe(2);
});

it("expires quarantined events on the same clock", async () => {
  await seed([DAY - 200, DAY], { event_day: 0 });
  expect(await count("quarantine")).toBe(2);
  await expire(env, NOW);
  expect(await count("quarantine")).toBe(1);
});

it("stops at the per-run delete cap, and the next run continues", async () => {
  const old = Array.from({ length: 7 }, (_, i) => event(i + 1, { event_day: DAY - 300 }));
  await ingest(body(old), { now: (DAY - 300) * DAY_MS });
  const capped = { ...env, RETENTION_DELETE_ROWS_PER_RUN: "5" } as Env;
  expect((await expire(capped, NOW)).deleted).toBe(5);
  expect(await count("events")).toBe(2);
  expect((await expire(capped, NOW)).deleted).toBe(2);
  expect(await count("events")).toBe(0);
});

it("runs from the cron trigger", async () => {
  await seed([DAY - 400]);
  const controller = createScheduledController({ scheduledTime: new Date(NOW), cron: "17 3 * * *" });
  await worker.scheduled(controller, env);
  expect(await count("events")).toBe(0);
});
