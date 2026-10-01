# Telemetry ingestion Worker

The public, anonymous endpoint `hanten`'s uploader sends telemetry to: a Cloudflare
Worker with one D1 database. It validates upload-v1 batches, stores each event once,
expires it after 180 days, and holds the SQL for the questions telemetry answers.
The contract is [`contracts/telemetry/upload-v1/`](../../contracts/telemetry/upload-v1/README.md);
the policy is [`docs/telemetry-strategy.md`](../../docs/telemetry-strategy.md).

```text
POST /v1/events
  200  { accepted, duplicate, rejected: [{ event_id, code }] }
  400  body over 262,144 bytes, bad JSON, invalid envelope, repeated event_id — never retry
  429  rate_limited                       Retry-After: 60
       daily_limit                        Retry-After: seconds to the next UTC day
  503  ingestion_disabled, storage_full   Retry-After: 3600
       unavailable (D1 or binding error)  Retry-After: 60
```

## The rules it keeps

- **Validation is the contract's schema file**, imported, never copied. Per event the
  code is the first of `unsupported_version`, `out_of_range` (a numeric bound only),
  `invalid_field`; an event passing all three from a release in `blocked_releases`
  is `release_blocked`. The contract's whole corpus runs through the Worker in
  `test/contract.test.ts`.
- **Answers come after the commit.** One D1 batch (a transaction) inserts every event
  and raises the counters; an insert returns its id only if the id is new to both
  `events` and `quarantine`, so a resent batch comes back `duplicate`.
- **The ceilings are CHECK constraints** on `daily_usage` and `storage`, raised inside
  that batch, so concurrent requests cannot overshoot them: a batch that would pass
  one fails whole. Storage counts rows (triggers keep it), not file size, since D1
  does not shrink its file when retention deletes.
- **Accepted is not analysed.** Three kinds of event go to `quarantine`, which no
  query reads; the client still sees them accepted: a day outside
  `[today − 180, today + 1]` (`event_day`), a release not in `allowed_releases`
  (`release`), and a cohort (release × OS × arch) past `MAX_COHORT_EVENTS_PER_DAY`
  stored events today (`cohort_volume`, a soft limit).
- **Nothing identifying is kept.** No IP, header or raw body is stored, days are the
  only time unit, and Worker logs are off (`wrangler.jsonc`, held by
  `scripts/check-config.mjs`). The rate limiter keys on the IP inside Cloudflare
  only. Cloudflare is the processor.

## Operating it

Every control is a row, changed without a deploy through the Cloudflare connector,
the D1 REST API, or the dashboard's D1 console (which takes no parameters: write the
value in for `?1`):

| To                                                    | Run                                                           |
| ----------------------------------------------------- | ------------------------------------------------------------- |
| stop ingestion (clients get 503 and keep their queue) | `UPDATE control SET value = '0' WHERE key = 'ingest_enabled'` |
| resume                                                | `UPDATE control SET value = '1' WHERE key = 'ingest_enabled'` |
| analyse a new release (best before shipping it)       | `INSERT INTO allowed_releases (nc_version) VALUES ('0.2.0')`  |
| …and recover what it sent before that                 | `queries/promote_release.sql` with `?1 = '0.2.0'`             |
| refuse a release for good (a bad build, fabrications) | `INSERT INTO blocked_releases (nc_version) VALUES ('0.1.1')`  |
| see volume against the ceilings, and the quarantine   | `queries/operations.sql`                                      |

`0001_init.sql` lists `0.1.0`, the crate's version since development began, so
development runs are analysed while `Cargo.toml` says `0.1.0`; an event carries no
dev marker. Before a release ships as `0.1.0`, delete those events.

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
| `promote_release.sql`         | moves a newly allowlisted release out of quarantine                           |

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
wire and **~1.14 KB** stored per event with its indexes. An analysed insert writes
**5 rows** (table, 2 indexes, the storage and cohort triggers), a quarantined one 3,
each request 1 more (`daily_usage`). A retention delete measured 2 locally; the
model assumes **4** (table, 2 indexes, trigger), as Cloudflare bills index rows.
Validating a 100-event batch takes ~7 ms of CPU.

| Per month              | Low (10 events/day) | Expected (300/day) | Worst (at every ceiling)                                                                                                                                                    |
| ---------------------- | ------------------- | ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Requests               | 300                 | 9 k                | 0.6 M accepted (20,000 events/day, one per request)                                                                                                                         |
| Rows written           | 3 k                 | 90 k               | 20 k × 30 × (5 + 1 + 4) = **6.0 M**                                                                                                                                         |
| Rows read              | negligible          | < 1 M              | < 100 M, plus queries (each a scan of ≤ 1.75 M rows)                                                                                                                        |
| CPU-ms                 | negligible          | < 0.1 M            | ≤ 60 M (the 100 ms cap × 0.6 M; ~6 M at the measured rate)                                                                                                                  |
| Storage after 180 days | 2 MB                | 62 MB              | capped at **1.75 M events ≈ 2 GB** (`MAX_STORED_EVENTS`, then 503)                                                                                                          |
| **Marginal cost**      | **$0**              | **$0**             | **$0** inside the included usage; **≤ $9** if the account's other projects had already used all of it ($6.00 writes, $1.50 storage, $1.20 CPU, $0.18 requests, $0.10 reads) |

The worst case is bounded by the ceilings: `MAX_EVENTS_PER_DAY` 20,000,
`MAX_BYTES_PER_DAY` 50 MB, `MAX_STORED_EVENTS` 1.75 M, and `limits.cpu_ms` 100 per
request. `scripts/check-config.mjs` refuses a deploy that raises any of them past
the values this table was computed at — change both together, and ask for approval
first if the new worst case can pass **$10 / month**.

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

**Migrations only add** (tables, columns with defaults, indexes, rows), and never
edit a file already applied. The deploy applies them before the new Worker goes
live, so the Worker still serving must keep working on the new schema; a removal
waits for a later deploy, once no running version reads the thing removed.

**Deploy** is the manual _Deploy telemetry ingestion_ workflow
(`.github/workflows/telemetry-deploy.yml`). A real deploy runs from `main` only: it
re-runs the gates, checks the account holds this config's D1 database, applies
pending migrations, reports the kill switch in the job summary, deploys, runs
`scripts/smoke.mjs` against the live URL, and rolls the Worker back if that fails. A
dry run gets no credentials and may run from any branch. The credentials are
`CLOUDFLARE_API_TOKEN` (Workers Scripts: Edit, D1: Edit, Account Settings: Read) and
`CLOUDFLARE_ACCOUNT_ID`, best kept as secrets of the `telemetry-production`
environment with deployments restricted to `main`. **Manual rollback:**
`wrangler rollback`; a migration is rolled forward with a new one.
