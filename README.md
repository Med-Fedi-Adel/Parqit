# parqit

Synthetic observability pipeline in Rust: generate HTTP logs, store them as Parquet, upload to MinIO, query with DataFusion. The project exists to learn how columnar storage and object-store query engines behave at scale, not to ship production infrastructure.

All data is synthetic. Query patterns mirror real log analytics: dashboards, incident drills, trace lookup, full retention scans.

**Stack:** Arrow (in-memory) → Parquet (on-disk) → MinIO (S3 API) → DataFusion (SQL)

---

## What we built

| Crate / tool | Role |
|--------------|------|
| **generator** | Synthetic logs → Arrow batches → Parquet (`legacy` v2 or `raw` v3 tiers) |
| **compact** | Merge micro-batch files into fewer, larger Parquet files |
| **inspect** | Read Parquet footers (row groups, stats, encodings) |
| **query** | SQL against MinIO; `--workload` runs predefined queries |
| **benchmark** | Naive JSONL baseline (v2), workload suite (v3), concurrent load (Step 4) |
| **workloads** | SQL templates in `queries/` filled from `manifest.json` |

Deeper design notes: [tech_spec.md](tech_spec.md) (v2), [tech_spec_v3.md](tech_spec_v3.md) (v3 plan).

---

## The process

We built this in two phases. v2 validated the mechanics on a small dataset. v3 stressed the same stack the way production observability pipelines actually look: many files, many days, compaction, and concurrent readers.

### Part 1: v2 baseline (5M rows, one day)

**What we did**

1. Generated 5M synthetic log rows with eight columns (`timestamp`, `service`, `status_code`, `latency_ms`, `trace_id`, `http_method`, `route`, `region`).
2. Wrote two layouts:
   - **Layout A (Hive):** `date/hour/service` partitions, 120 files, one per partition.
   - **Layout B (flat):** single `part-000.parquet` for row-group pruning without partition overhead.
3. Inspected Parquet metadata to confirm row groups, dictionary encoding, and min/max statistics.
4. Uploaded to MinIO and registered tables in DataFusion.
5. Ran EXPLAIN on selective queries to see partition pruning and column projection in the physical plan.
6. Benchmarked naive JSONL scan vs DataFusion pushdown.

**Numbers (v2, 5M rows)**

| Metric | Value |
|--------|------:|
| JSON → Parquet compression | 4.18× |
| Naive JSONL selective scan | 12,582 ms |
| DataFusion tight partition query | 19.8 ms |
| Hive full scan vs flat full scan | 56 ms vs 5 ms |

**Sub-conclusion:** Columnar engines win by reading less, not by parsing faster. Partition pruning and projection are real and measurable even at 5M rows. Hive layout loses on full scan (120 file opens) but wins when the query matches partition keys. That tradeoff carries through everything that follows.

```bash
make generate
make query-explain-hive
make bench-step1 && make bench-step2
```

Results: [results/benchmarks.md](results/benchmarks.md)

---

### Part 2: v3 scale (200M rows, seven days, micro-batch ingest)

**What we did**

1. Extended the generator with **tiers** (`smoke`, `standard`, `stress`) and a **raw ingest layout**:
   - 200M rows, 7 days, 20 services, Zipf-skewed traffic (exponent 1.2).
   - **15-minute micro-batches:** 4 Parquet files per hour per service → **13,440 files** under `data/raw/`.
   - Added `pod` and `environment` columns (medium/low cardinality, like real K8s metadata).
2. Wrote `manifest.json` at generate time with one `trace_id` per day for reproducible lookup benchmarks.
3. Generated on disk (~9.4 GiB raw), then uploaded to MinIO (`s3://logs/raw/`).

**Sub-conclusion:** At 200M rows the bottleneck shifts from "can Parquet skip row groups?" to "how many objects does the query engine have to list and open?" File count becomes a first-class cost. v2's 120 partitions were a toy; v3's 13,440 files resemble a week of micro-batch ingest.

```bash
make generate-v3-standard    # ~9.4 GiB raw, takes a while
make upload-v3-raw             # see scripts/upload-v3.sh
make verify-minio              # local vs MinIO file counts
```

---

### Part 3: Compaction (raw → compacted)

**What we did**

1. Built the **compact** crate to merge all `part-batch-*.parquet` files within each `date/hour/service` partition.
2. Wrote compacted output to `data/compacted/` (~9.3 GiB, **3,360 files**, one per partition).
3. Uploaded compacted layout to `s3://logs/compacted/`.
4. Verified row counts match raw exactly (200M in, 200M out).

Compaction does not change the data or partition scheme. It only reduces file count and footer/metadata overhead.

**Sub-conclusion:** Compaction is not about compression ratio (bytes stay roughly the same). It trades **file sprawl for fewer, larger objects** so list-heavy queries spend less time on object-store housekeeping. The interesting comparisons are raw vs compacted on the same queries.

```bash
make compact
make upload-v3-compacted
```

Report: [results/compaction.json](results/compaction.json)

---

### Part 4: Workload benchmarks (nine production-style queries)

**What we did**

1. Defined nine SQL workloads in `queries/` (dashboard, incident drill, trace lookup, cross-day report, and others).
2. Filled templates from `manifest.json` (dates, trace IDs).
3. Ran each workload against **raw** and **compacted** in MinIO, median of 3 runs.
4. Saved results to [results/benchmarks_v3.md](results/benchmarks_v3.md).

**Numbers (v3 standard tier, 200M rows, single-threaded)**

| Workload | Raw (ms) | Compacted (ms) | Notes |
|----------|----------:|---------------:|-------|
| Tight partition (1 file) | 71 | 49 | Partition pruning hero |
| Dashboard (1h errors) | 347 | 296 | Matches partition keys |
| Incident (15m 5xx) | 246 | 219 | Scoped time + filter |
| Scoped day (payments 5xx) | 443 | 380 | Single service, full day |
| Full scan | 2,252 | 1,785 | List/metadata bound |
| Selective 5xx (all data) | 26,011 | 12,813 | **2× faster compacted** |
| Trace lookup | 94,321 | 81,412 | No index; scans everything |
| Cross-day report | 16,682 | 16,565 | I/O bound, not file-count bound |
| Projection wide | 76,685 | 78,445 | Reads many columns; compacted ~same |

**Sub-conclusion:** Scoped queries that hit partition keys stay in tens to hundreds of milliseconds even at 200M rows. Queries that touch the whole dataset expose file-count tax: selective 5xx without a partition filter is 2× faster on compacted. Trace lookup is honestly slow (~81–94 s) because nothing in the folder layout helps find a UUID. That is the expected production outcome without a secondary index.

```bash
make bench-v3-all
# or one at a time:
cargo run -p query -- --path compacted --workload dashboard
```

---

### Part 5: Concurrent load (8 workers, mixed queries)

**What we did**

1. Ran a **mixed workload** under concurrency: dashboard, incident, tight partition, scoped day, full scan (5 types, 8 parallel workers, 3 rounds = 24 queries per layout).
2. Measured **p50 / p95 / p99** latency (typical vs tail under contention).
3. Compared raw vs compacted on the same concurrent mix.

**Numbers (8 workers × 3 rounds)**

| Layout | p50 (ms) | p95 (ms) | p99 (ms) |
|--------|----------:|---------:|---------:|
| Raw | 1,315 | 13,895 | 15,780 |
| Compacted | 1,024 | 5,278 | 5,768 |

Scoped queries under load land around **1 s** (vs 200–400 ms isolated). Tail latency is dominated by **full scan** colliding in the same round; round 1 on raw hit ~16 s cold full scans before caches warmed.

**Sub-conclusion:** Single-threaded benchmarks understate production pain. Under 8-way load, partition-matched queries degrade ~3–4× but remain usable. Full scans and cold cache dominate p95/p99. Compaction cuts concurrent tail latency roughly in half (p99 5.8 s vs 15.8 s raw). Plan for warmup and avoid full retention scans on the raw ingest layout in a busy cluster.

```bash
make bench-v3-step4
```

---

## How columnar search works (three skips)

These stack in every query engine that reads Parquet well:

1. **Partition pruning** — skip whole `date/hour/service` directories when the filter matches path keys.
2. **Column projection** — read only columns in the SELECT list, not the full row.
3. **Row group pruning** — skip groups using min/max statistics in the Parquet footer.

A query that matches all three on Hive-partitioned data can open **one file**, read **one column**, and skip row groups. A naive JSONL scan still parses every line.

---

## Quick start

**Prerequisites:** Rust, Docker (MinIO)

**v2 demo (5M rows, fast):**

```bash
make generate
make minio-up
# Copy layout_a/ and layout_b/ into s3://logs/ (MinIO mc or AWS CLI)
make demo
```

**v3 pipeline (standard tier; plan ~20 GiB for raw + compacted):**

```bash
make generate-v3-standard
make compact
make minio-up
make upload-v3
make verify-minio          # expect raw=13440, compacted=3360
make bench-v3-all
make bench-v3-step4
make demo                  # compacted incident drill (auto-detects v3)
```

If MinIO file counts drift after re-compaction: `make clean-minio-v3 && make upload-v3`.

**Useful commands:** `make help`

---

## Schema

**v2 layouts** (`layout_a`, `layout_b`): 8 columns; `service` in partition path for Hive layout.

**v3 raw/compacted** adds:

| Column | Notes |
|--------|-------|
| `pod` | Synthetic K8s pod name, ~200 per service |
| `environment` | `prod` (~99%) or `staging` |

Example path: `data/raw/date=2026-09-01/hour=14/service=payments/part-batch-0002.parquet`

---

## Repo layout

```
crates/
  generator/   inspect/   query/   benchmark/   compact/   workloads/
queries/                 # SQL workload templates
scripts/
  demo.sh  upload-v3.sh  verify-minio.sh  clean-minio-v3.sh  save-explain.sh
results/
  benchmarks.md       # v2
  benchmarks_v3.md    # v3 workloads + Step 4 concurrency
  compaction.json
docker-compose.yml    Makefile
```

---

## Conclusion

We started with a 5M-row toy dataset and proved that Parquet + DataFusion pushdown works: a tight partition query ran in **19.8 ms** where naive JSON took **12.6 seconds**. That was the easy part.

v3 asked what changes when the dataset looks like production: **200 million rows**, **seven days of retention**, **13,440 micro-batch files**, skewed services, and queries that do not all align with partition keys. Three lessons stood out:

1. **File count is a query cost.** Full scan and unscoped filters on raw ingest are list-bound. Compaction merged 13,440 files into 3,360 with the same bytes and the same rows, and selective 5xx dropped from 26 s to 13 s. Under concurrent load, compacted p99 was **5.8 s vs 15.8 s** on raw.

2. **Partition layout is a query contract.** When filters match `date`, `hour`, and `service`, latency stays in tens of milliseconds even at 200M rows. When they do not (trace lookup, cross-day aggregates), the engine does honest full work. No folder scheme fixes high-cardinality point lookups; you need an index or a different store.

3. **Benchmark in isolation and under load.** Single-threaded medians flatter the system. Eight parallel clients turned 300 ms dashboard queries into ~1 s and made cold full scans the tail-latency story. That is closer to what a shared MinIO bucket sees in practice.

The stack held up: same generator, same SQL, same MinIO, same DataFusion. What changed was the **shape of the data on disk** and the **honesty of the benchmark suite**. Columnar engines still win by reading less. At real scale you also have to manage **how much metadata you pay before you read anything**.

---

## References

- v2 benchmarks: [results/benchmarks.md](results/benchmarks.md)
- v3 benchmarks: [results/benchmarks_v3.md](results/benchmarks_v3.md)
- v3 plan: [tech_spec_v3.md](tech_spec_v3.md)
