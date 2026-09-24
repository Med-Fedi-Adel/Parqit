SELECT route, AVG(latency_ms) AS avg_ms, MAX(latency_ms) AS max_ms
FROM logs
WHERE service = 'api'
  AND date >= '{{start_date}}' AND date <= '{{last_date}}'
GROUP BY route
ORDER BY avg_ms DESC
LIMIT 20
