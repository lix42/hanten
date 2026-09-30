// Deploy-time ceilings from wrangler.jsonc `vars`. A missing or malformed value
// throws, so a bad deploy fails every request loudly rather than running unbounded.

export interface Limits {
  maxEventsPerDay: number;
  maxBytesPerDay: number;
  maxCohortEventsPerDay: number;
  maxDbBytes: number;
  retentionDays: number;
  retentionDeleteRowsPerRun: number;
}

export function limits(env: Env): Limits {
  return {
    maxEventsPerDay: positive(env.MAX_EVENTS_PER_DAY, "MAX_EVENTS_PER_DAY"),
    maxBytesPerDay: positive(env.MAX_BYTES_PER_DAY, "MAX_BYTES_PER_DAY"),
    maxCohortEventsPerDay: positive(env.MAX_COHORT_EVENTS_PER_DAY, "MAX_COHORT_EVENTS_PER_DAY"),
    maxDbBytes: positive(env.MAX_DB_BYTES, "MAX_DB_BYTES"),
    retentionDays: positive(env.RETENTION_DAYS, "RETENTION_DAYS"),
    retentionDeleteRowsPerRun: positive(env.RETENTION_DELETE_ROWS_PER_RUN, "RETENTION_DELETE_ROWS_PER_RUN"),
  };
}

function positive(raw: string | undefined, name: string): number {
  const n = Number(raw);
  if (!Number.isSafeInteger(n) || n <= 0) throw new Error(`${name} must be a positive integer`);
  return n;
}

export const MS_PER_DAY = 86_400_000;

/** UTC days since the epoch: the only time resolution the service keeps. */
export function dayOf(ms: number): number {
  return Math.floor(ms / MS_PER_DAY);
}
