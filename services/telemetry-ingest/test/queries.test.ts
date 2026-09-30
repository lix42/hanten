// The checked-in queries over seeded events, ingested through the handler.
import { env } from "cloudflare:test";
import { beforeEach, expect, it } from "vitest";
import failureRate from "../queries/failure_rate_by_release.sql?raw";
import failuresByStage from "../queries/failures_by_stage.sql?raw";
import operations from "../queries/operations.sql?raw";
import panics from "../queries/panics.sql?raw";
import timing from "../queries/timing_by_cohort.sql?raw";
import { body, corpus, DAY, event, ingest, reset } from "./helpers";

type Event = ReturnType<typeof event>;

function failure(
  id: number,
  stage: string,
  error_kind: string,
  exit_code: number,
  patch: Record<string, unknown> = {},
): Event {
  const e = event(id, { stage, ...patch });
  e.outcome = { status: "failure", exit_code, error_kind, warning_bucket: "0" };
  delete e.image;
  delete e.conversion;
  e.timing_ms = { total: 5 };
  return e;
}

function success(id: number, total: number, patch: Record<string, unknown> = {}): Event {
  const e = event(id, patch);
  (e.timing_ms as Record<string, number>).total = total;
  return e;
}

function panic(id: number, frame: string): Event {
  const p = (corpus.validRequests.find((c) => c.name === "panic")!.request as { events: Event[] }).events[0]!;
  return { ...structuredClone(p), event_id: event(id).event_id, frames: [frame] };
}

const linux = { platform: { os: "linux", arch: "x86_64", cpu_bucket: "16" } };

beforeEach(async () => {
  await reset();
  await env.DB.prepare("INSERT INTO allowed_releases (nc_version) VALUES ('0.2.0')").run();
  const r = await ingest(
    body([
      success(1, 100),
      success(2, 200),
      success(3, 300),
      success(4, 400, linux),
      failure(5, "decode", "decode", 3),
      failure(6, "decode", "decode", 3),
      failure(7, "encode", "write", 5, linux),
      success(8, 1000, { nc_version: "0.2.0" }),
      failure(9, "setup", "usage", 2, { nc_version: "0.2.0" }),
      panic(10, "nc::io::decode"),
      panic(11, "nc::io::decode"),
      panic(12, "nc::pipeline::look"),
      // Quarantined: never counted.
      failure(13, "decode", "decode", 3, { event_day: 0 }),
    ]),
  );
  expect(r.status).toBe(200);
});

async function run(sql: string, ...params: unknown[]) {
  return (
    await env.DB.prepare(sql)
      .bind(...params)
      .all()
  ).results;
}

it("failure rate per release and platform", async () => {
  expect(await run(failureRate, DAY)).toEqual([
    { nc_version: "0.1.0", os: "linux", arch: "x86_64", events: 2, failures: 1, failure_rate: 0.5 },
    { nc_version: "0.1.0", os: "macos", arch: "aarch64", events: 5, failures: 2, failure_rate: 0.4 },
    { nc_version: "0.2.0", os: "macos", arch: "aarch64", events: 2, failures: 1, failure_rate: 0.5 },
  ]);
  expect(await run(failureRate, DAY + 1)).toEqual([]);
});

it("failures by stage and error kind", async () => {
  expect(await run(failuresByStage, DAY)).toEqual([
    { nc_version: "0.1.0", stage: "decode", error_kind: "decode", exit_code: 3, failures: 2, share_of_release: 0.2857 },
    { nc_version: "0.1.0", stage: "encode", error_kind: "write", exit_code: 5, failures: 1, share_of_release: 0.1429 },
    { nc_version: "0.2.0", stage: "setup", error_kind: "usage", exit_code: 2, failures: 1, share_of_release: 0.5 },
  ]);
});

it("timing percentiles per release and cohort", async () => {
  expect(await run(timing, DAY, "total", "os")).toEqual([
    { nc_version: "0.1.0", cohort: "linux", events: 1, min_ms: 400, p50_ms: 400, p90_ms: 400, max_ms: 400 },
    { nc_version: "0.1.0", cohort: "macos", events: 3, min_ms: 100, p50_ms: 200, p90_ms: 300, max_ms: 300 },
    { nc_version: "0.2.0", cohort: "macos", events: 1, min_ms: 1000, p50_ms: 1000, p90_ms: 1000, max_ms: 1000 },
  ]);
  // A stage's timing and the megapixel cohort (the corpus event is 18.7 MP).
  const decode = await run(timing, DAY, "decode", "megapixels");
  expect(decode).toEqual([
    { nc_version: "0.1.0", cohort: "10_25_mp", events: 4, min_ms: 412, p50_ms: 412, p90_ms: 412, max_ms: 412 },
    { nc_version: "0.2.0", cohort: "10_25_mp", events: 1, min_ms: 412, p50_ms: 412, p90_ms: 412, max_ms: 412 },
  ]);
  for (const cohort of ["encoding", "arch", "cpu_bucket", "input_size"]) {
    const rows = await run(timing, DAY, "total", cohort);
    expect(
      rows.every((r) => r.cohort !== null),
      cohort,
    ).toBe(true);
  }
});

it("panics per release and first frame", async () => {
  const rows = await run(panics, DAY);
  expect(rows.map((r) => [r.first_frame, r.panics])).toEqual([
    ["nc::io::decode", 2],
    ["nc::pipeline::look", 1],
  ]);
});

it("operations: volume against the ceilings and the quarantine", async () => {
  expect(await run(operations, DAY)).toEqual([
    {
      day: DAY,
      requests: 1,
      events: 13,
      bytes: expect.any(Number),
      quarantined_event_day: 1,
      quarantined_cohort_volume: 0,
    },
  ]);
});
