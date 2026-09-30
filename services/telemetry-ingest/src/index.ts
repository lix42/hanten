// Nothing here logs: request data must not reach Cloudflare's logs
// (wrangler.jsonc turns observability off; scripts/check-config.mjs holds it).
import { handleEvents } from "./ingest";
import { expire } from "./retention";

export default {
  async fetch(request, env): Promise<Response> {
    const { pathname } = new URL(request.url);
    if (pathname !== "/v1/events") return Response.json({ error: "not_found" }, { status: 404 });
    if (request.method !== "POST") {
      return Response.json({ error: "method_not_allowed" }, { status: 405, headers: { allow: "POST" } });
    }
    return handleEvents(request, env, Date.now());
  },

  async scheduled(controller, env): Promise<void> {
    await expire(env, controller.scheduledTime);
  },
} satisfies ExportedHandler<Env>;
