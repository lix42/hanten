-- Telemetry upload-v1 storage. Times are UTC days since the epoch only; no IP,
-- header or raw body is ever stored. Every index is a write per insert and per
-- retention delete, so add one only for a query that needs it (README "Cost model").

-- Analytical events: schema-valid, allowlisted, not quarantined.
CREATE TABLE events (
  event_id              TEXT    PRIMARY KEY,
  received_day          INTEGER NOT NULL,
  event_day             INTEGER NOT NULL,
  source_schema_version INTEGER NOT NULL,
  event_name            TEXT    NOT NULL,
  nc_version            TEXT    NOT NULL,
  os                    TEXT    NOT NULL,
  arch                  TEXT    NOT NULL,
  cpu_bucket            TEXT    NOT NULL,
  stage                 TEXT    NOT NULL,
  status                TEXT,
  error_kind            TEXT,
  exit_code             INTEGER,
  encoding              TEXT,
  megapixels_tenths     INTEGER,
  input_size_bucket     TEXT,
  total_ms              INTEGER,
  payload               TEXT    NOT NULL  -- the validated event, re-serialized
) STRICT, WITHOUT ROWID;

CREATE INDEX events_received_day ON events (received_day);
CREATE INDEX events_release ON events (nc_version, event_name, status);

-- Accepted but kept out of analysis: anomalous or suspicious-volume events. An
-- event_id lives in exactly one of the two tables.
CREATE TABLE quarantine (
  event_id     TEXT    PRIMARY KEY,
  received_day INTEGER NOT NULL,
  reason       TEXT    NOT NULL,
  payload      TEXT    NOT NULL
) STRICT, WITHOUT ROWID;

CREATE INDEX quarantine_received_day ON quarantine (received_day);

-- Operational switches, changed with a D1 query rather than a deploy (README).
CREATE TABLE control (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

INSERT INTO control (key, value) VALUES ('ingest_enabled', '1');

-- Releases whose events are accepted; any other nc_version is `release_blocked`.
CREATE TABLE allowed_releases (
  nc_version TEXT PRIMARY KEY
) STRICT;

INSERT INTO allowed_releases (nc_version) VALUES ('0.1.0');

-- Per-day counters for the cost ceilings.
CREATE TABLE daily_usage (
  day      INTEGER PRIMARY KEY,
  requests INTEGER NOT NULL DEFAULT 0,
  events   INTEGER NOT NULL DEFAULT 0,
  bytes    INTEGER NOT NULL DEFAULT 0
) STRICT;

-- Per-day, per-cohort counters for the suspicious-volume quarantine.
CREATE TABLE cohort_usage (
  day        INTEGER NOT NULL,
  nc_version TEXT    NOT NULL,
  os         TEXT    NOT NULL,
  arch       TEXT    NOT NULL,
  events     INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (day, nc_version, os, arch)
) STRICT, WITHOUT ROWID;
