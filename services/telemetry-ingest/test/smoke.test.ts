import { env } from "cloudflare:test";
import { expect, it } from "vitest";
import { self } from "./helpers";

it("boots with the migrations applied", async () => {
  expect((await self.fetch("https://x/v1/nope")).status).toBe(404);
  expect(await env.DB.prepare("SELECT count(*) AS n FROM allowed_releases").first()).toEqual({ n: 1 });
});
