SELECT service, status_code, COUNT(*) AS n
FROM logs
WHERE date = '{{incident_date}}' AND hour = '{{incident_hour}}'
  AND timestamp >= CAST('{{incident_ts_start}}' AS TIMESTAMP)
  AND timestamp < CAST('{{incident_ts_end}}' AS TIMESTAMP)
  AND status_code >= 500
GROUP BY service, status_code
ORDER BY service, status_code
