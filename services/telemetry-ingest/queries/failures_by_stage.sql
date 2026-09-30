-- Where conversions fail: per release, the failed stage and error kind, with its
-- share of that release's conversions. Events received on or after day ?1.
WITH totals AS (
  SELECT nc_version, COUNT(*) AS events
  FROM events
  WHERE event_name = 'conversion' AND received_day >= ?1
  GROUP BY nc_version
)
SELECT e.nc_version, e.stage, e.error_kind, e.exit_code,
       COUNT(*)                            AS failures,
       ROUND(1.0 * COUNT(*) / t.events, 4) AS share_of_release
FROM events AS e JOIN totals AS t USING (nc_version)
WHERE e.event_name = 'conversion' AND e.status = 'failure' AND e.received_day >= ?1
GROUP BY e.nc_version, e.stage, e.error_kind, e.exit_code
ORDER BY e.nc_version, failures DESC, e.stage, e.error_kind;
