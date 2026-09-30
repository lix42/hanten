// The daily cron: delete events and quarantined events received more than
// RETENTION_DAYS ago, and the counters with them. Deletes are writes (row plus
// each index), so one run stops at RETENTION_DELETE_ROWS_PER_RUN and the next
// run continues.
import { dayOf, limits } from "./config";

const CHUNK = 5_000;

export async function expire(env: Env, nowMs: number): Promise<{ deleted: number }> {
  const cfg = limits(env);
  const cutoff = dayOf(nowMs) - cfg.retentionDays;
  let deleted = 0;
  for (const table of ["events", "quarantine"] as const) {
    while (deleted < cfg.retentionDeleteRowsPerRun) {
      const n = Math.min(CHUNK, cfg.retentionDeleteRowsPerRun - deleted);
      const r = await env.DB.prepare(
        `DELETE FROM ${table} WHERE event_id IN
           (SELECT event_id FROM ${table} WHERE received_day < ?1 LIMIT ?2)
         RETURNING event_id`,
      )
        .bind(cutoff, n)
        .all();
      // Counted from RETURNING: meta.changes also counts the storage trigger's writes.
      deleted += r.results.length;
      if (r.results.length < n) break;
    }
  }
  await env.DB.batch([
    env.DB.prepare(`DELETE FROM daily_usage WHERE day < ?1`).bind(cutoff),
    env.DB.prepare(`DELETE FROM cohort_usage WHERE day < ?1`).bind(cutoff),
  ]);
  return { deleted };
}
