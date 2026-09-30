-- Telemetry upload-v1 storage. Times are UTC days since the epoch only; no IP,
-- header or raw body is ever stored. Every index and trigger write is a billed
-- row per insert and per delete (README "Cost model").

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

-- Accepted but kept out of analysis. `reason` is 'event_day', 'cohort_volume' or
-- 'release' (a release not yet allowlisted; queries/promote_release.sql moves it).
-- An event_id lives in at most one of the two tables.
CREATE TABLE quarantine (
  event_id     TEXT    PRIMARY KEY,
  received_day INTEGER NOT NULL,
  reason       TEXT    NOT NULL,
  nc_version   TEXT    NOT NULL,
  payload      TEXT    NOT NULL
) STRICT, WITHOUT ROWID;

CREATE INDEX quarantine_received_day ON quarantine (received_day);

-- Operational switches, changed with a D1 query rather than a deploy (README).
CREATE TABLE control (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

INSERT INTO control (key, value) VALUES ('ingest_enabled', '1');

-- Releases whose events are analysed. Any other release is quarantined, unless
-- blocked: a blocked release's events are rejected (`release_blocked`) for good.
CREATE TABLE allowed_releases (
  nc_version TEXT PRIMARY KEY
) STRICT;

INSERT INTO allowed_releases (nc_version) VALUES ('0.1.0');

CREATE TABLE blocked_releases (
  nc_version TEXT PRIMARY KEY
) STRICT;

-- scripts/smoke.mjs posts this release: it must be rejected, never stored.
INSERT INTO blocked_releases (nc_version) VALUES ('0.0.0-smoke');

-- The ceilings are CHECK constraints, so concurrent requests cannot overshoot
-- them: each write batch raises the counter, and a batch that would pass a limit
-- fails whole. The Worker writes the limits from its vars with every batch.

-- Per-day request volume against MAX_EVENTS_PER_DAY and MAX_BYTES_PER_DAY.
CREATE TABLE daily_usage (
  day        INTEGER PRIMARY KEY,
  requests   INTEGER NOT NULL,
  events     INTEGER NOT NULL,
  bytes      INTEGER NOT NULL,
  max_events INTEGER NOT NULL,
  max_bytes  INTEGER NOT NULL,
  CONSTRAINT daily_limit CHECK (events <= max_events AND bytes <= max_bytes)
) STRICT;

-- Rows held in events + quarantine, against MAX_STORED_EVENTS. A row count, not
-- the file size: D1 does not shrink its file when retention deletes rows.
CREATE TABLE storage (
  id         INTEGER PRIMARY KEY CHECK (id = 1),
  events     INTEGER NOT NULL,
  max_events INTEGER NOT NULL,
  CONSTRAINT storage_limit CHECK (events <= max_events)
) STRICT;

INSERT INTO storage (id, events, max_events) VALUES (1, 0, 0);

CREATE TRIGGER events_stored AFTER INSERT ON events
BEGIN UPDATE storage SET events = events + 1 WHERE id = 1; END;
CREATE TRIGGER events_expired AFTER DELETE ON events
BEGIN UPDATE storage SET events = events - 1 WHERE id = 1; END;
CREATE TRIGGER quarantine_stored AFTER INSERT ON quarantine
BEGIN UPDATE storage SET events = events + 1 WHERE id = 1; END;
CREATE TRIGGER quarantine_expired AFTER DELETE ON quarantine
BEGIN UPDATE storage SET events = events - 1 WHERE id = 1; END;

-- Analysed events per day and cohort, for the suspicious-volume quarantine.
-- Counted by trigger, so only rows actually stored count: a resent duplicate
-- does not.
CREATE TABLE cohort_usage (
  day        INTEGER NOT NULL,
  nc_version TEXT    NOT NULL,
  os         TEXT    NOT NULL,
  arch       TEXT    NOT NULL,
  events     INTEGER NOT NULL,
  PRIMARY KEY (day, nc_version, os, arch)
) STRICT, WITHOUT ROWID;

CREATE TRIGGER events_cohort AFTER INSERT ON events
BEGIN
  INSERT INTO cohort_usage (day, nc_version, os, arch, events)
  VALUES (NEW.received_day, NEW.nc_version, NEW.os, NEW.arch, 1)
  ON CONFLICT (day, nc_version, os, arch) DO UPDATE SET events = events + 1;
END;
