# Benchmark Results (Day 3)

_Synthetic dataset, 5 million HTTP log rows._

## Summary

**Headline numbers:**

| Metric | Value |
|--------|------:|
| Compression (JSON → Parquet) | **4.18×** smaller |
| Naive JSONL selective scan | **12,582 ms** |
| DataFusion Hive (partition + filter) | **19.8 ms** |
| Speedup vs naive JSON | **~635×** |

**Three optimizations stacking:**

1. **Partition pruning**: Hive opens 1 of 120 files when `date`, `hour`, `service` are filtered
2. **Column projection**: narrow query reads ~48 KB of `status_code`; wide query with `trace_id` is ~3.4× slower
3. **Row group statistics**: Parquet footer min/max enable pushdown (limited here because 2xx and 5xx share row groups)

**Key tradeoff:** Hive full scan (56 ms) is slower than flat (5 ms) because listing 120 files has overhead. Partitioning wins when queries match partition keys, but it loses when you scan everything.

**One-liner:** Columnar engines win by reading less, not by searching faster.


## Step 3.1: Compression + naive baseline

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

_Note: naive scan reads and parses every line even when filtering; no pushdown._

## Step 3.2: DataFusion via MinIO (median of 3 runs)

| Scenario | Latency (ms) | vs naive selective |
|----------|-------------:|-------------------:|
| DataFusion full scan (Hive) | 56.3 | 224× faster |
| DataFusion full scan (flat) | 5.3 | 2,375× faster |
| DataFusion selective 5xx (Hive) | 328.4 | 38× faster |
| DataFusion selective 5xx (flat) | 164.4 | 77× faster |
| **DataFusion tight partition (Hive)** | **19.8** | **635× faster** |
| DataFusion payments 5xx all day (flat) | 299.0 | 42× faster |
| DataFusion projection narrow (Hive) | 395.0 | 32× faster |
| DataFusion projection wide (Hive) | 1338.4 | 9× faster |

_Source: Parquet in MinIO (`s3://logs/`). Compare to Step 3.1 naive JSONL baseline._
