SELECT COUNT(*) AS errors
FROM logs
WHERE date = '{{last_date}}' AND service = 'payments' AND status_code >= 500
