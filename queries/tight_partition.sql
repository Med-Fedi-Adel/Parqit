SELECT COUNT(*) AS n
FROM logs
WHERE date = '{{incident_date}}' AND hour = '{{incident_hour}}' AND service = 'payments' AND status_code >= 500
