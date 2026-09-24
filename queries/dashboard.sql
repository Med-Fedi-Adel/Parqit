SELECT service, SUM(CASE WHEN status_code >= 500 THEN 1 ELSE 0 END) AS errors
FROM logs
WHERE date = '{{last_date}}' AND hour = '{{last_hour}}'
GROUP BY service
ORDER BY service
