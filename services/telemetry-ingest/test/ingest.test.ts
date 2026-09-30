import { env } from "cloudflare:test";
import { beforeEach, describe, expect, it } from "vitest";
import { MAX_BODY_BYTES } from "../src/contract";
import { body, count, DAY, event, hexId, ingest, NOW, post, reset, self, type Reply } from "./helpers";

beforeEach(reset);

describe("deduplication", () => {
  it("answers a replayed event_id duplicate and keeps one row", async () => {
    const first = await (await ingest(body([event(1)]))).json<Reply>();
    expect(first.accepted).toEqual([hexId(1)]);
    const again = await (await ingest(body([event(1), event(2)]))).json<Reply>();
    expect(again).toMatchObject({ accepted: [hexId(2)], duplicate: [hexId(1)], rejected: [] });
    expect(await count("events")).toBe(2);
  });

  it("makes a retry after a lost response safe", async () => {
    // The first response is dropped after the commit; the client resends the batch.
    await ingest(body([event(1), event(2)]));
    const retry = await (await ingest(body([event(1), event(2)]))).json<Reply>();
    expect(retry).toMatchObject({ accepted: [], duplicate: [hexId(1), hexId(2)] });
    expect(await count("events")).toBe(2);
  });

  it("answers duplicate for an id already quarantined, in either direction", async () => {
    await ingest(body([event(1, { event_day: 0 })]));
    expect(await count("quarantine")).toBe(1);
    const r = await (await ingest(body([event(1)]))).json<Reply>();
    expect(r.duplicate).toEqual([hexId(1)]);
    expect(await count("events")).toBe(0);

    await ingest(body([event(2)]));
    const back = await (await ingest(body([event(2, { event_day: 0 })]))).json<Reply>();
    expect(back.duplicate).toEqual([hexId(2)]);
    expect(await count("quarantine")).toBe(1);
  });

  it("refuses a batch naming one event_id twice with 400", async () => {
    const res = await ingest(body([event(1), event(1)]));
    expect(res.status).toBe(400);
    expect(await count("events")).toBe(0);
  });

  it("stores a partial batch: the good events land, the bad one is rejected", async () => {
    const r = await (await ingest(body([event(1), event(2, { stage: "nope" }), event(3)]))).json<Reply>();
    expect(r).toMatchObject({
      accepted: [hexId(1), hexId(3)],
      rejected: [{ event_id: hexId(2), code: "invalid_field" }],
    });
    expect(await count("events")).toBe(2);
  });
});

describe("release allowlist", () => {
  it("rejects a schema-valid event from an unlisted release and stores nothing", async () => {
    const r = await (await ingest(body([event(1, { nc_version: "9.9.9" })]))).json<Reply>();
    expect(r.rejected).toEqual([{ event_id: hexId(1), code: "release_blocked" }]);
    expect(await count("events")).toBe(0);
    expect(await count("quarantine")).toBe(0);
  });

  it("ranks every schema rejection above release_blocked", async () => {
    const r = await (await ingest(body([event(1, { nc_version: "9.9.9", event_day: -1 })]))).json<Reply>();
    expect(r.rejected).toEqual([{ event_id: hexId(1), code: "out_of_range" }]);
  });
});

describe("quarantine", () => {
  it("keeps an event dated outside [today - retention, today + 1] out of analysis", async () => {
    const r = await (
      await ingest(
        body([event(1, { event_day: DAY + 2 }), event(2, { event_day: DAY - 181 }), event(3, { event_day: DAY + 1 })]),
      )
    ).json<Reply>();
    expect(r.accepted).toEqual([hexId(1), hexId(2), hexId(3)]);
    const q = await env.DB.prepare("SELECT event_id, reason FROM quarantine ORDER BY event_id").all();
    expect(q.results).toEqual([
      { event_id: hexId(1), reason: "event_day" },
      { event_id: hexId(2), reason: "event_day" },
    ]);
    expect(await count("events")).toBe(1);
  });

  it("sends a cohort's events past its daily volume to quarantine", async () => {
    const opts = { env: { MAX_COHORT_EVENTS_PER_DAY: "3" } };
    await ingest(body([event(1), event(2)]), opts);
    const r = await (
      await ingest(
        body([event(3), event(4), event(5, { platform: { os: "linux", arch: "x86_64", cpu_bucket: "8" } })]),
        opts,
      )
    ).json<Reply>();
    expect(r.accepted).toEqual([hexId(3), hexId(4), hexId(5)]);
    expect(await count("events")).toBe(4); // 1, 2, 3 and the other cohort's 5
    const q = await env.DB.prepare("SELECT event_id, reason FROM quarantine").all();
    expect(q.results).toEqual([{ event_id: hexId(4), reason: "cohort_volume" }]);
  });
});

describe("kill switch and ceilings", () => {
  it("refuses everything with 503 while ingestion is disabled", async () => {
    await env.DB.prepare("UPDATE control SET value = '0' WHERE key = 'ingest_enabled'").run();
    const res = await ingest(body([event(1)]));
    expect(res.status).toBe(503);
    expect(res.headers.get("retry-after")).toBe("3600");
    expect(await count("events")).toBe(0);
  });

  it("refuses a batch that would pass the daily event ceiling with 429 until tomorrow", async () => {
    const opts = { env: { MAX_EVENTS_PER_DAY: "3" } };
    expect((await ingest(body([event(1), event(2)]), opts)).status).toBe(200);
    const res = await ingest(body([event(3), event(4)]), opts);
    expect(res.status).toBe(429);
    expect(res.headers.get("retry-after")).toBe(String(12 * 3600));
    expect((await ingest(body([event(3)]), opts)).status).toBe(200);
    expect(await count("events")).toBe(3);
    // The next day starts a fresh budget.
    expect((await ingest(body([event(4, { event_day: DAY + 1 })]), { ...opts, now: NOW + 86_400_000 })).status).toBe(
      200,
    );
  });

  it("refuses past the daily byte ceiling with 429", async () => {
    const one = JSON.stringify(body([event(1)])).length;
    const opts = { env: { MAX_BYTES_PER_DAY: String(one * 2) } };
    expect((await ingest(body([event(1)]), opts)).status).toBe(200);
    expect((await ingest(body([event(2)]), opts)).status).toBe(200);
    expect((await ingest(body([event(3)]), opts)).status).toBe(429);
    expect(await count("events")).toBe(2);
  });

  it("refuses with 503 once the database passes its size ceiling", async () => {
    const res = await ingest(body([event(1)]), { env: { MAX_DB_BYTES: "1" } });
    expect(res.status).toBe(503);
    expect(await res.json()).toEqual({ error: "storage_full" });
    expect(await count("events")).toBe(0);
  });

  it("counts a refused request against nothing", async () => {
    expect((await ingest(body([event(1), event(2)]), { env: { MAX_EVENTS_PER_DAY: "1" } })).status).toBe(429);
    expect(await count("daily_usage")).toBe(0);
  });

  it("fails closed with 503 on a malformed ceiling", async () => {
    expect((await ingest(body([event(1)]), { env: { MAX_EVENTS_PER_DAY: "lots" } })).status).toBe(503);
    expect(await count("events")).toBe(0);
  });

  it("answers 503, retryable, when D1 fails mid-request", async () => {
    await env.DB.prepare("DROP TABLE cohort_usage").run();
    try {
      const res = await ingest(body([event(1)]));
      expect(res.status).toBe(503);
      expect(res.headers.get("retry-after")).toBe("60");
    } finally {
      await env.DB.prepare(
        "CREATE TABLE cohort_usage (day INTEGER NOT NULL, nc_version TEXT NOT NULL, os TEXT NOT NULL, arch TEXT NOT NULL, events INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (day, nc_version, os, arch)) STRICT, WITHOUT ROWID",
      ).run();
    }
  });

  it("rolls the whole batch back when one write fails", async () => {
    await env.DB.prepare("DROP TABLE cohort_usage").run();
    await env.DB.prepare(
      "CREATE TABLE cohort_usage (day INTEGER NOT NULL, nc_version TEXT NOT NULL, os TEXT NOT NULL, arch TEXT NOT NULL, events INTEGER NOT NULL CHECK (events < 0), PRIMARY KEY (day, nc_version, os, arch)) STRICT, WITHOUT ROWID",
    ).run();
    try {
      expect((await ingest(body([event(1)]))).status).toBe(503);
      expect(await count("events")).toBe(0);
      expect(await count("daily_usage")).toBe(0);
    } finally {
      await env.DB.prepare("DROP TABLE cohort_usage").run();
      await env.DB.prepare(
        "CREATE TABLE cohort_usage (day INTEGER NOT NULL, nc_version TEXT NOT NULL, os TEXT NOT NULL, arch TEXT NOT NULL, events INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (day, nc_version, os, arch)) STRICT, WITHOUT ROWID",
      ).run();
    }
  });
});

describe("malformed requests", () => {
  it("refuses a body over the cap, declared or streamed, with 400", async () => {
    const big = "x".repeat(MAX_BODY_BYTES + 1);
    expect((await ingest(null, { raw: big })).status).toBe(400);
    const streamed = new Request("https://ingest.test/v1/events", {
      method: "POST",
      body: new Blob([big]).stream(),
      // @ts-expect-error: workerd needs duplex for a stream body
      duplex: "half",
    });
    const res = await self.fetch(streamed);
    expect(res.status).toBe(400);
  });

  it("accepts a body exactly at the cap's edge", async () => {
    const b = JSON.stringify(body([event(1)]));
    const padded = b.slice(0, -1) + " ".repeat(MAX_BODY_BYTES - b.length) + "}";
    expect(padded.length).toBe(MAX_BODY_BYTES);
    expect((await ingest(null, { raw: padded })).status).toBe(200);
  });

  it("refuses unparseable JSON and invalid UTF-8 with 400", async () => {
    expect((await ingest(null, { raw: "{" })).status).toBe(400);
    const bad = new Request("https://ingest.test/v1/events", {
      method: "POST",
      body: new Uint8Array([0x7b, 0xff, 0x7d]),
    });
    expect((await self.fetch(bad)).status).toBe(400);
  });

  it("answers only POST /v1/events", async () => {
    expect((await self.fetch("https://ingest.test/v1/events")).status).toBe(405);
    expect((await self.fetch("https://ingest.test/")).status).toBe(404);
    expect((await self.fetch("https://ingest.test/v1/events/x", { method: "POST" })).status).toBe(404);
  });
});

describe("rate limit", () => {
  it("answers 429 once one address passes the coarse limit", async () => {
    const statuses: number[] = [];
    for (let i = 0; i < 31; i++) statuses.push((await post(body([event(i + 1)]), { ip: "192.0.2.7" })).status);
    expect(statuses.slice(0, 30).every((s) => s === 200)).toBe(true);
    expect(statuses[30]).toBe(429);
    // Another address is unaffected.
    expect((await post(body([event(99)]), { ip: "192.0.2.8" })).status).toBe(200);
  });
});

describe("privacy", () => {
  it("stores no address, header or unvalidated byte anywhere", async () => {
    await post(body([event(1), event(2, { nc_version: "9.9.9" })]), {
      ip: "203.0.113.99",
      headers: { "user-agent": "secret-agent/1.0", "x-extra": "canary-header" },
    });
    const tables = await env.DB.prepare(
      "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_cf_%' AND name != 'd1_migrations'",
    ).all<{ name: string }>();
    expect(tables.results.map((t) => t.name).sort()).toEqual([
      "allowed_releases",
      "cohort_usage",
      "control",
      "daily_usage",
      "events",
      "quarantine",
    ]);
    let dump = "";
    for (const { name } of tables.results)
      dump += JSON.stringify((await env.DB.prepare(`SELECT * FROM ${name}`).all()).results);
    for (const canary of ["203.0.113.99", "secret-agent", "canary-header", "9.9.9"]) expect(dump).not.toContain(canary);
  });

  it("stores the payload exactly as validated", async () => {
    const e = event(1);
    await ingest(body([e]));
    const row = await env.DB.prepare("SELECT payload FROM events").first<{ payload: string }>();
    expect(JSON.parse(row!.payload)).toEqual(e);
  });

  it("keeps time at day granularity", async () => {
    await ingest(body([event(1)]));
    const row = await env.DB.prepare("SELECT received_day, event_day FROM events").first();
    expect(row).toEqual({ received_day: DAY, event_day: DAY });
  });
});
