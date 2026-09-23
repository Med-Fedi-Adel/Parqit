# parqit

Small Rust workspace that generates fake HTTP logs, stores them as Parquet, and queries them with DataFusion over MinIO. Built to get hands-on with Arrow/Parquet pushdown, not to be production infrastructure.

All data is synthetic (5M rows). Query shapes are meant to resemble log analytics.

## What's in here

Four crates, one job each:

- **generator** — builds Arrow batches, writes Parquet. Same dataset twice: hive partitions under `data/layout_a/` (`date/hour/service`, 120 files) and one flat file at `data/layout_b/part-000.parquet`.
- **inspect** — reads Parquet footers (row groups, encodings, min/max stats). Useful before trusting EXPLAIN output.
- **query** — registers tables from MinIO (`s3://logs/`) and runs SQL. Supports `--explain` against both layouts.
- **benchmark** — exports flat Parquet to JSONL, times a naive line-by-line scan, then runs the DataFusion scenarios in `results/benchmarks.md`.

Typical path: generate locally → upload to MinIO → query/benchmark from object storage. DataFusion reads Parquet back into Arrow internally; the generator is the only place Arrow shows up explicitly in code.

## Why queries get fast

Three separate skips, and they compose:

**Partition pruning** (layout A only). Directories are named `date=…/hour=…/service=…/`. Filter on those columns and DataFusion never opens the other 119 files.

**Column projection.** `SELECT status_code` reads one column chunk, not the whole row. Adding `trace_id` roughly triples I/O on the same filter (see benchmarks).

**Row group pruning.** Each Parquet file stores min/max per column per row group. The reader can skip groups that cannot match the predicate. Limited here because 2xx and 5xx often land in the same group.

A tight hive query (one hour, one service, filter on `status_code`) opened a single file and finished in ~20 ms. Scanning the equivalent JSONL took ~12.6 s because every line gets parsed.

## Quick start

Needs Rust and Docker (for MinIO).

```bash
make generate
make minio-up
# upload data/layout_a and layout_b to s3://logs/ (once)
make demo
```

Other useful targets:

```bash
make inspect-partition     # one hive file's metadata
make query-explain-hive    # EXPLAIN with partition + filter
make bench-step1           # JSONL export, compression, naive scan
make bench-step2           # DataFusion timings via MinIO
make help
```

## Numbers worth knowing

From [results/benchmarks.md](results/benchmarks.md), 5M rows, median of 3 runs:

| Scenario | Latency |
|----------|--------:|
| Naive JSONL selective scan | 12,582 ms |
| DataFusion hive (partition + filter) | 19.8 ms |
| JSON → Parquet size | 4.18× smaller |

Full scan is faster on the flat file (5 ms vs 56 ms hive) because listing 120 objects has overhead. Partitioning pays off when the query matches the folder layout.

## Schema

| Column | Layout A | Layout B |
|--------|----------|----------|
| `timestamp` | file | file |
| `service` | partition path | file |
| `status_code` | file | file |
| `latency_ms` | file | file |
| `trace_id` | file | file |
| `http_method` | file | file |
| `route` | file | file |
| `region` | file | file |

Layout A example: `data/layout_a/date=2026-09-01/hour=14/service=payments/part-000.parquet`

Layout B: `data/layout_b/part-000.parquet` (~50 row groups of 100k rows)

## Repo layout

```
crates/generator   crates/inspect   crates/query   crates/benchmark
scripts/demo.sh    scripts/save-explain.sh
results/benchmarks.md   results/explain/
docker-compose.yml      Makefile
```

## Partitioning choice

Went with `date/hour/service` for layout A. Fits "errors in the last hour for payments" without creating thousands of tiny files. Tradeoff: anything that touches the whole day pays for 120 file opens. At larger scale I'd partition primarily by time, run compaction on small parts, and leave high-cardinality lookups (`trace_id`) to something other than folder names.

More background in [tech_spec.md](tech_spec.md). Planned v3 (production scale, ingest sprawl, compaction, concurrent workloads): [tech_spec_v3.md](tech_spec_v3.md).
