# parqit

Mini observability pipeline for learning **Arrow**, **Parquet**, and **DataFusion** on synthetic HTTP logs from generation through columnar storage to SQL query.

> **Note:** All data is **synthetic** (5M generated rows). The query patterns mirror real log analytics.

## Architecture

```
Synthetic generator (Rust)
        │
        ▼
  Arrow RecordBatches          ← in-memory columnar (RAM)
        │
        ▼
  Parquet files (Layout A/B)   ← on-disk columnar + statistics
        │
        ▼
  MinIO (S3-compatible)        ← object storage (disk, remote API)
        │
        ▼
  DataFusion SQL engine        ← reads Parquet → Arrow → execute
        │
        ▼
  Benchmarks + EXPLAIN
```

**Data flow:** Generator builds **Arrow** batches in RAM, writes **Parquet** to disk, copies to **MinIO**, DataFusion reads Parquet back into **Arrow** to run SQL. See [results/benchmarks.md](results/benchmarks.md) for numbers.

## How columnar search works

Columnar engines don't "search faster"; they **read less data**:

1. **Partition pruning**: skip whole directories (`date=…/hour=…/service=…/`) when the query filters on those keys
2. **Column projection**: read only the columns referenced in the query (`status_code`, not `trace_id`)
3. **Row group pruning**: skip row groups using min/max statistics in the Parquet footer

These stack. A selective query on Hive-partitioned data can open **1 file**, read **one column**, and skip row groups that can't match, instead of scanning every row of a JSON file.

## Quick start

### Prerequisites

- Rust (stable)
- Docker (MinIO)

### One-command demo

```bash
make minio-up          # start MinIO (first time)
# upload data to MinIO once (see Day 2 upload steps)
make demo              # inspect → EXPLAIN → query → benchmark summary
```

### Day 1: Generate and inspect

```bash
make generate          # 5M rows → data/layout_a + data/layout_b
make inspect-partition # row groups, stats, encodings
make data-stats        # file counts and sizes
```

### Day 2: Query via MinIO

```bash
make minio-up
make query                       # COUNT(*) on Hive layout
make query-explain-hive          # show pushdown in EXPLAIN
make query-both                  # compare logs vs logs_flat
```

### Day 3: Benchmarks

```bash
make bench-step1       # JSON export + compression + naive baseline
make bench-step2       # DataFusion benchmarks (requires MinIO + upload)
cat results/benchmarks.md
```

Run `make help` for all targets.

## Benchmark highlights

From [results/benchmarks.md](results/benchmarks.md) (5M synthetic rows, median of 3 runs):

| Scenario | Latency |
|----------|--------:|
| Naive JSONL selective scan | 12,582 ms |
| DataFusion Hive (partition + filter) | **19.8 ms** |
| Compression JSON → Parquet | **4.18×** |

## Schema

| Column | Layout A (Hive) | Layout B (flat) | Notes |
|--------|-----------------|-----------------|-------|
| `timestamp` | in file | in file | Microsecond UTC |
| `service` | **path only** | in file | Hive partition key |
| `status_code` | in file | in file | ~90% 2xx, ~5% 4xx, ~5% 5xx |
| `latency_ms` | in file | in file | Higher for errors |
| `trace_id` | in file | in file | UUID (high cardinality) |
| `http_method` | in file | in file | Dictionary-friendly |
| `route` | in file | in file | ~30 API paths |
| `region` | in file | in file | 4 AWS regions |

**Layout A:** `data/layout_a/date=2026-09-01/hour=HH/service=NAME/part-000.parquet` (120 partitions)

**Layout B:** `data/layout_b/part-000.parquet` (single file, 50 row groups)

## Project layout

```
crates/
  generator/    # synthetic data → Arrow → Parquet
  inspect/      # read Parquet footer metadata
  query/        # DataFusion + MinIO
  benchmark/    # naive JSON baseline + DataFusion benchmarks
scripts/
  demo.sh       # end-to-end demo
  save-explain.sh
results/
  benchmarks.md
  explain/
```

## Partitioning tradeoffs

**Chosen:** `date/hour/service` Hive-style (120 partitions for 5M rows).

| Benefit | Cost |
|---------|------|
| Partition pruning on time + service queries | 120 files to list on full scan |
| Matches observability query patterns | Small files (~42k rows each) |
| Complements row group pruning | More metadata overhead vs one big file |

**At real scale:** partition by query patterns (usually time), compact small files, stream ingestion (Kafka → batch writer), add metadata indexes for high-cardinality columns like `trace_id`.

## What I learned

- **Arrow** is the in-memory column format; **Parquet** is the on-disk format; **DataFusion** converts between them during query execution
- Parquet's power is **reading less**: column projection and partition pruning matter as much as compression
- Row group statistics enable pushdown but only when min/max ranges are tight enough to skip groups
- Hive partitioning helps scoped queries but hurts full-table scans (120 files vs 1)
- Object storage (MinIO/S3) adds latency; pushdown matters even more when I/O is remote

See [tech_spec.md](tech_spec.md) for architecture notes and build schedule.
