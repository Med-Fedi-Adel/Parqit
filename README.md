# parqit

Mini observability pipeline for learning Arrow, Parquet, and DataFusion — synthetic HTTP logs from generation through columnar storage to SQL query.

## Prerequisites

- Rust (stable)
- Docker (Day 2 — MinIO)

## Quick start (Day 1)

Generate 5 million synthetic log rows:

```bash
cargo run -p generator -- --rows 5000000 --output data
```

This writes two layouts under `data/`:

- **Layout A (Hive):** `data/layout_a/date=2026-09-01/hour=HH/service=NAME/part-000.parquet` — 120 partitions (24 hours × 5 services)
- **Layout B (flat):** `data/layout_b/part-000.parquet` — single file, all columns in-file

Inspect Parquet metadata (row groups, min/max statistics, encodings):

```bash
cargo run -p inspect -- data/layout_a --limit 3
cargo run -p inspect -- data/layout_b/part-000.parquet
```

### Generator options

```bash
cargo run -p generator -- --help

# Smaller test run
cargo run -p generator -- --rows 100000 --output data/test
```

| Flag | Default | Description |
|------|---------|-------------|
| `--rows` | 5_000_000 | Total log rows |
| `--output` | `data` | Output directory |
| `--row-group-size` | 100_000 | Parquet row group size |
| `--seed` | 42 | RNG seed for reproducibility |

### Schema

| Column | In Layout A file | In Layout B file | Notes |
|--------|-----------------|------------------|-------|
| `timestamp` | yes | yes | Microsecond UTC |
| `service` | path only | yes | Hive partition key in Layout A |
| `status_code` | yes | yes | ~90% 2xx, ~5% 4xx, ~5% 5xx |
| `latency_ms` | yes | yes | Higher for errors |
| `trace_id` | yes | yes | UUID |
| `http_method` | yes | yes | GET, POST, … |
| `route` | yes | yes | ~30 API paths |
| `region` | yes | yes | 4 AWS regions |

## Project layout

```
crates/
  generator/   # synthetic data → Arrow → Parquet
  inspect/     # read Parquet footer metadata
  query/       # DataFusion + MinIO (Day 2)
```

See [plan.md](plan.md) for the full build schedule and interview demo plan.
