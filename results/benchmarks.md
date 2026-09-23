# Benchmark Results (Day 3)

## Step 3.1 — Compression + naive baseline

| Format | Size |
|--------|------|
| JSON Lines | 994.74 MB |
| Parquet (flat) | 238.19 MB |
| Parquet (hive) | 236.65 MB |
| **JSON / flat ratio** | **4.18x** |

## Naive JSONL scan (median of 3 runs)

| Scenario | Latency (ms) | Rows |
|----------|-------------|------|
| Full scan | 12310.6 | 5000000 |
| Selective (`status_code >= 500`) | 12582.2 | 250284 matched |

_Note: naive scan reads and parses every line even when filtering — no pushdown._



## Step 3.2 — DataFusion via MinIO (median of 3 runs)

| Scenario | Latency (ms) |
|----------|-------------:|
| DataFusion full scan (Hive) | 56.3 |
| DataFusion full scan (flat) | 5.3 |
| DataFusion selective 5xx (Hive) | 328.4 |
| DataFusion selective 5xx (flat) | 164.4 |
| DataFusion tight partition (Hive) | 19.8 |
| DataFusion payments 5xx all day (flat) | 299.0 |
| DataFusion projection narrow (Hive) | 395.0 |
| DataFusion projection wide (Hive) | 1338.4 |

_Source: Parquet in MinIO (`s3://logs/`). Compare to Step 3.1 naive JSONL baseline._
