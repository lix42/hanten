// The whole upload-v1 corpus through the Worker's real entry point.
import { env } from "cloudflare:test";
import { beforeEach, describe, expect, it } from "vitest";
import { isValidResponse } from "../src/contract";
import { corpus, hexId, post, reset, type Reply } from "./helpers";

let next = 1;
/** The case with every event_id replaced by a fresh one, so cases never collide. */
function fresh(request: unknown): { request: unknown; ids: string[] } {
  const r = structuredClone(request) as { events: { event_id: string }[] };
  const ids = r.events.map((e) => (e.event_id = hexId(next++)));
  return { request: r, ids };
}

beforeEach(async () => {
  await reset();
  // Every release the valid corpus names.
  await env.DB.batch(
    ["0.0.0", "0.2.0-rc.1", "1.22.333-rc.1.x-y.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"].map((v) =>
      env.DB.prepare("INSERT INTO allowed_releases (nc_version) VALUES (?1)").bind(v),
    ),
  );
});

describe("valid requests", () => {
  for (const c of corpus.validRequests) {
    it(`accepts ${c.name}`, async () => {
      const { request, ids } = fresh(c.request);
      const res = await post(request);
      expect(res.status).toBe(200);
      const reply = await res.json<Reply>();
      expect(isValidResponse(reply)).toBe(true);
      expect(reply).toEqual({ upload_schema_version: 1, accepted: ids, duplicate: [], rejected: [] });
    });
  }
});

describe("invalid requests", () => {
  for (const c of corpus.invalidRequests) {
    it(`answers ${c.name} with ${c.expect}`, async () => {
      if (c.expect === "http_400") {
        const res = await post(c.request);
        expect(res.status).toBe(400);
        return;
      }
      const original = (c.request as { events: { event_id: string }[] }).events;
      // The bad event keeps a recognisable id; any other event gets a fresh one.
      const bad = original.length === 1 ? 0 : original.findIndex((e) => e.event_id === hexId(9));
      const { request, ids } = fresh(c.request);
      const res = await post(request);
      expect(res.status).toBe(200);
      const reply = await res.json<Reply>();
      expect(isValidResponse(reply)).toBe(true);
      expect(reply.rejected).toEqual([{ event_id: ids[bad], code: c.expect }]);
      expect(reply.accepted).toEqual(ids.filter((_, i) => i !== bad));
      expect(reply.duplicate).toEqual([]);
    });
  }
});

describe("response fixtures", () => {
  it("accepts every valid response and rejects every invalid one", () => {
    for (const c of corpus.validResponses) expect(isValidResponse(c.response), c.name).toBe(true);
    for (const c of corpus.invalidResponses) expect(isValidResponse(c.response), c.name).toBe(false);
  });
});
