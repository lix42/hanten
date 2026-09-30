-- Ingestion volume per day against the ceilings, what was quarantined, and the
-- rows held against MAX_STORED_EVENTS, from day ?1. Not an analytical query.
SELECT u.day, u.requests, u.events, u.bytes, u.max_events, u.max_bytes,
       (SELECT COUNT(*) FROM quarantine AS q WHERE q.received_day = u.day AND q.reason = 'event_day')     AS quarantined_event_day,
       (SELECT COUNT(*) FROM quarantine AS q WHERE q.received_day = u.day AND q.reason = 'release')       AS quarantined_release,
       (SELECT COUNT(*) FROM quarantine AS q WHERE q.received_day = u.day AND q.reason = 'cohort_volume') AS quarantined_cohort_volume,
       (SELECT events FROM storage)                                                                        AS stored_events
FROM daily_usage AS u
WHERE u.day >= ?1
ORDER BY u.day;
