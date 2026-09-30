# Telemetry ingestion Worker

The public, anonymous endpoint `hanten`'s uploader sends telemetry to: a Cloudflare
Worker with one D1 database. It validates upload-v1 batches, stores each event once,
expires it after 180 days, and holds the SQL for the questions telemetry answers.
The contract is [`contracts/telemetry/upload-v1/`](../../contracts/telemetry/upload-v1/README.md);
the policy is [`docs/telemetry-strategy.md`](../../docs/telemetry-strategy.md).

```text
POST /v1/events
  429  rate limited, or a daily ceiling reached (Retry-After: seconds to next UTC day)
  400  body over 262,144 bytes, bad JSON, invalid envelope, repeated event_id
  503  kill switch off, storage ceiling reached, D1 failure (Retry-After)
  200  { accepted, duplicate, rejected: [{ event_id, code }] }
```

## The rules it keeps

- **Validation is the contract's schema file**, imported, never copied. Per event the
  code is the first of `unsupported_version`, `out_of_range` (a numeric bound only),
  `invalid_field`; an event passing all three but from a release missing from
  `allowed_releases` is `release_blocked`. The contract's whole corpus runs through
  the Worker in `test/contract.test.ts`.
- **Answers come after the commit.** One D1 batch (a transaction) inserts every event
  and the counters; an insert returns its id only if the id is new to both `events`
  and `quarantine`, so a resent batch comes back `duplicate` and stores nothing.
- **Accepted is not analysed.** An event dated outside `[today − 180, today + 1]`, or a
  cohort (release × OS × arch) past `MAX_COHORT_EVENTS_PER_DAY` today, goes to
  `quarantine`. The client sees it accepted; no query reads it.
- **Nothing identifying is kept.** No IP, header or raw body is stored, days are the
  only time unit, and Worker logs are off (`wrangler.jsonc`, held by
  `scripts/check-config.mjs`). The rate limiter keys on the IP inside Cloudflare
  only. Cloudflare is the processor.

## Operating it

Every control is a row, changed without a deploy — through the Cloudflare
connector, the dashboard's D1 console, or
`wrangler d1 execute hanten-telemetry --remote --command "…"`:

| To                                                                                        | Run                                                           |
| ----------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| stop ingestion (clients get 503 and keep their queue)                                     | `UPDATE control SET value = '0' WHERE key = 'ingest_enabled'` |
| resume                                                                                    | `UPDATE control SET value = '1' WHERE key = 'ingest_enabled'` |
| accept a new release — **before** shipping it; its events are otherwise rejected for good | `INSERT INTO allowed_releases (nc_version) VALUES ('0.2.0')`  |
| see volume against the ceilings                                                           | `queries/operations.sql`                                      |

The kill switch stops storage, not billing: the Worker still answers each request.
To stop requests reaching it at all, deploy with `"workers_dev": false`.

## Queries

`queries/*.sql`, each tested over seeded events in `test/queries.test.ts`. `?1` is
the first received day (UTC days since the epoch: `SELECT unixepoch() / 86400 - 30`
is 30 days back).

| File                          | Answers                                                                       |
| ----------------------------- | ----------------------------------------------------------------------------- |
| `failure_rate_by_release.sql` | conversion failure rate per release and platform                              |
| `failures_by_stage.sql`       | per release, the failing stage and error kind, as a share of its conversions  |
| `timing_by_cohort.sql`        | p50/p90 of total or one stage's time, per release and one cohort (`?2`, `?3`) |
| `panics.sql`                  | panics per release, stage and first frame                                     |
| `operations.sql`              | daily volume and quarantine counts                                            |

**Reading the numbers.** Every count describes unverified events from opt-in
clients: not users, not installs, not the population. There is no identifier, so
repeat activity is indistinguishable from breadth, and a valid-looking event may be
fabricated. State that beside every rate you report.

## Cost model

The account is on the Workers Paid plan (the strategy's 2026-09-30 amendment).
Included each month, **shared with every other Worker and D1 database on the
account**: 10 M requests, 30 M CPU-ms, 25 B D1 rows read, 50 M rows written, 5 GB
storage. Beyond that: $0.30 / M requests, $0.02 / M CPU-ms, $0.001 / M rows read,
$1.00 / M rows written, $0.75 / GB-month.

Measured locally (`workerd`, the corpus's full-success event): **768 bytes** on the
wire and **~1.14 KB** stored per event, including its two indexes; an insert writes
**3 rows** (table + 2 indexes), a retention delete the same. Validating a 100-event
batch takes ~7 ms of CPU.

| Per month                                   | Low (10 events/day) | Expected (300/day) | Worst (at every ceiling)                                                                                                                                                    |
| ------------------------------------------- | ------------------- | ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Requests                                    | 300                 | 9 k                | 0.6 M accepted (20,000 events/day, one per request)                                                                                                                         |
| Rows written (inserts, counters, retention) | 2 k                 | 50 k               | 20 k × 30 × (3 + 2 + 3) = **4.8 M**                                                                                                                                         |
| Rows read                                   | negligible          | < 1 M              | < 100 M, plus queries (each a scan, ≤ 3.6 M rows)                                                                                                                           |
| CPU-ms                                      | negligible          | < 0.1 M            | ≤ 120 M (the 200 ms cap × 0.6 M; ~6 M at the measured rate)                                                                                                                 |
| Storage after 180 days                      | 2 MB                | 62 MB              | capped at **2 GB** (`MAX_DB_BYTES`, then 503)                                                                                                                               |
| **Marginal cost**                           | **$0**              | **$0**             | **$0** inside the included usage; **≤ $9** if the account's other projects had already used all of it ($4.80 writes, $2.40 CPU, $1.50 storage, $0.18 requests, $0.10 reads) |

The worst case is bounded by the ceilings: `MAX_EVENTS_PER_DAY` 20,000,
`MAX_BYTES_PER_DAY` 50 MB, `MAX_DB_BYTES` 2 GB, and `limits.cpu_ms` 200 per request.
`scripts/check-config.mjs` refuses a deploy that raises any of them past the values
this table was computed at — change both together, and ask for approval first if
the new worst case can pass **$10 / month**.

**The one dimension the Worker cannot cap is requests it refuses.** A flood of bad
or rate-limited requests still bills per request ($0.30 / M past the included 10 M)
and a little CPU. The per-IP rate limit (30 / minute) and Cloudflare's DDoS
protection absorb most of it; set a billing notification in the dashboard
(Manage Account → Notifications → Usage-based billing) as the backstop, and use
`"workers_dev": false` to take the endpoint down.

## Develop, test, deploy

```sh
pnpm install
pnpm verify        # prettier, tsc, check-config, then vitest in workerd with a local D1
pnpm types         # after changing wrangler.jsonc
```

A schema change is a new file in `migrations/`, never an edit to an applied one.

**Deploy** is the manual _Deploy telemetry ingestion_ workflow
(`.github/workflows/telemetry-deploy.yml`, from `main` only): it re-runs the gates,
applies pending migrations, deploys, then runs `scripts/smoke.mjs` against the live
URL. It needs the `CLOUDFLARE_API_TOKEN` (Workers Scripts: Edit, D1: Edit, Account
Settings: Read) and `CLOUDFLARE_ACCOUNT_ID` repository secrets. **Rollback:**
`wrangler rollback` restores the previous Worker version; a migration is rolled
forward with a new one.
