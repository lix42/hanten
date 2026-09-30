-- Move a release's quarantined events into analysis once it is in
-- allowed_releases (?1 = its nc_version). Run both statements, in order; each is
-- safe to rerun, and until the second runs the Worker still answers `duplicate`.
INSERT INTO events (event_id, received_day, event_day, source_schema_version, event_name,
  nc_version, os, arch, cpu_bucket, stage, status, error_kind, exit_code, encoding,
  megapixels_tenths, input_size_bucket, total_ms, payload)
SELECT event_id, received_day,
       json_extract(payload, '$.event_day'),
       json_extract(payload, '$.source_schema_version'),
       json_extract(payload, '$.event_name'),
       nc_version,
       json_extract(payload, '$.platform.os'),
       json_extract(payload, '$.platform.arch'),
       json_extract(payload, '$.platform.cpu_bucket'),
       json_extract(payload, '$.stage'),
       json_extract(payload, '$.outcome.status'),
       json_extract(payload, '$.outcome.error_kind'),
       json_extract(payload, '$.outcome.exit_code'),
       json_extract(payload, '$.conversion.encoding'),
       json_extract(payload, '$.image.megapixels_tenths'),
       json_extract(payload, '$.image.input_size_bucket'),
       json_extract(payload, '$.timing_ms.total'),
       payload
FROM quarantine
WHERE reason = 'release' AND nc_version = ?1
ON CONFLICT (event_id) DO NOTHING;

DELETE FROM quarantine
WHERE reason = 'release' AND nc_version = ?1
  AND event_id IN (SELECT event_id FROM events);
