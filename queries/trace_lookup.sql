SELECT timestamp, service, route, status_code, latency_ms
FROM logs
WHERE trace_id = '{{trace_id}}'
