-- Panics per release, active stage and first sanitized frame, received on or
-- after day ?1.
SELECT nc_version, stage,
       json_extract(payload, '$.frames[0]') AS first_frame,
       COUNT(*)                             AS panics
FROM events
WHERE event_name = 'panic' AND received_day >= ?1
GROUP BY nc_version, stage, first_frame
ORDER BY panics DESC, nc_version, stage, first_frame;
