SELECT AVG(latency_ms) AS avg_ms, MAX(trace_id) AS max_trace, MAX(route) AS max_route
FROM logs
WHERE status_code >= 500
