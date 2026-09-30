-- Conversion failure rate per release and platform, for events received on or
-- after day ?1 (UTC days since the epoch). Advisory: opt-in, unverified events,
-- never users or installs (README "Reading the numbers").
SELECT nc_version, os, arch,
       COUNT(*)                                               AS events,
       SUM(status = 'failure')                                AS failures,
       ROUND(1.0 * SUM(status = 'failure') / COUNT(*), 4)     AS failure_rate
FROM events
WHERE event_name = 'conversion' AND received_day >= ?1
GROUP BY nc_version, os, arch
ORDER BY nc_version, os, arch;
