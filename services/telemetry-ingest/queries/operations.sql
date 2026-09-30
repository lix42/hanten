-- Ingestion volume per day against the ceilings, and what was quarantined, from
-- day ?1. Not an analytical query: quarantined events are never mixed in.
SELECT u.day, u.requests, u.events, u.bytes,
       (SELECT COUNT(*) FROM quarantine AS q WHERE q.received_day = u.day AND q.reason = 'event_day')     AS quarantined_event_day,
       (SELECT COUNT(*) FROM quarantine AS q WHERE q.received_day = u.day AND q.reason = 'cohort_volume') AS quarantined_cohort_volume
FROM daily_usage AS u
WHERE u.day >= ?1
ORDER BY u.day;
