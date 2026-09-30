-- Timing distribution of successful conversions per release and one cohort.
--   ?1  first received day (UTC days since the epoch)
--   ?2  timing field: 'total' or a stage ('decode', 'encode', ...)
--   ?3  cohort: 'encoding', 'os', 'arch', 'cpu_bucket', 'megapixels' or 'input_size'
-- Percentiles are nearest-rank. Compare releases within one 'megapixels' or
-- 'input_size' cohort, never across them.
WITH t AS (
  SELECT nc_version,
         CASE ?3
           WHEN 'encoding'   THEN encoding
           WHEN 'os'         THEN os
           WHEN 'arch'       THEN arch
           WHEN 'cpu_bucket' THEN cpu_bucket
           WHEN 'input_size' THEN input_size_bucket
           WHEN 'megapixels' THEN CASE
             WHEN megapixels_tenths < 100 THEN 'lt_10_mp'
             WHEN megapixels_tenths < 250 THEN '10_25_mp'
             WHEN megapixels_tenths < 500 THEN '25_50_mp'
             ELSE '50_plus_mp' END
         END AS cohort,
         json_extract(payload, '$.timing_ms.' || ?2) AS ms
  FROM events
  WHERE event_name = 'conversion' AND status = 'success' AND received_day >= ?1
),
r AS (
  SELECT nc_version, cohort, ms,
         ROW_NUMBER() OVER (PARTITION BY nc_version, cohort ORDER BY ms) AS rn,
         COUNT(*)     OVER (PARTITION BY nc_version, cohort)             AS n
  FROM t
  WHERE ms IS NOT NULL
)
SELECT nc_version, cohort,
       n                                                  AS events,
       MIN(ms)                                            AS min_ms,
       MAX(CASE WHEN rn = (n + 1) / 2 THEN ms END)        AS p50_ms,
       MAX(CASE WHEN rn = (9 * n + 9) / 10 THEN ms END)   AS p90_ms,
       MAX(ms)                                            AS max_ms
FROM r
GROUP BY nc_version, cohort
ORDER BY nc_version, cohort;
