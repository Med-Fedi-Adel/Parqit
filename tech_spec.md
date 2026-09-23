# Mini Observability Pipeline: Project Plan (v2)

**Goal:** Build a small, self-contained pipeline that mirrors Tsuga's core architecture (ingest → process → index → query) using the exact tools from the job description: Rust, Arrow, Parquet, object storage, and DataFusion. The purpose is hands-on familiarity with columnar storage and query execution ahead of the CEO conversation, not to rebuild Tsuga.

**Scope:** Synthetic dataset, single machine, local object storage (MinIO). No dependency on any existing project.

**Timeline:** Tue Sep 22 → Sun Sep 28 (6 days, with Sunday as buffer / demo rehearsal).

**Repo name:** `parqit`, Parquet + query + it runs.

### Confirmed decisions

| Decision | Choice |
|----------|--------|
| Dataset size | **5 million rows** |
| Schema | **8 columns:** `timestamp`, `service`, `status_code`, `latency_ms`, `trace_id`, `http_method`, `route`, `region` |
| Enhancements | **All 9 kept** (metadata inspect, partitioning A/B, selectivity ladder, projection demo, workspace, demo script, richer schema, talk track) |
| Vortex | **Skipped for now**; can revisit if time allows after Day 4 |

| Decision | Status |
|----------|--------|
| Partition scheme (Layout A) | **`date/hour/service`** (option B) |


## Partitioning explained

Partitioning is about **how you lay out files on disk** before the query engine opens them. It's separate from what happens *inside* a Parquet file.

Think of it like organizing papers in filing cabinets vs. shoving everything in one drawer.

### Two levels of "skipping data"

```
Query: WHERE date = '2026-09-01' AND service = 'payments' AND status_code >= 500
```

**Level 1: Partition pruning (directory level)**

If files are stored like this:

```
logs/
  date=2026-09-01/
    service=payments/
      part-000.parquet
      part-001.parquet
    service=api/
      part-000.parquet
  date=2026-09-02/
    service=payments/
      part-000.parquet
```

DataFusion lists the bucket, sees the `date=` and `service=` path segments, and **never opens** files under `date=2026-09-02/` or `service=api/`. Whole directories are skipped based on your `WHERE` clause matching the folder names.

This is called **Hive-style partitioning**: the column values are encoded in the directory path, not inside the Parquet file.

**Level 2: Row group pruning (inside a file)**

Even after opening `part-000.parquet`, each file has internal **row groups** with min/max statistics per column. DataFusion skips row groups where, e.g., `status_code` max is 299 when you filter `status_code >= 500`.

**Both levels stack.** Partition pruning reduces how many files you open. Row group pruning reduces how much you read inside each file.

### Layout A vs Layout B in this project

| | Layout A (partitioned) | Layout B (flat) |
|--|----------------------|-----------------|
| Path example | `date=2026-09-01/service=payments/part-000.parquet` | `logs/part-000.parquet` |
| Partition pruning | Yes, skips irrelevant folders | No, must list/read all files |
| Row group pruning | Yes, inside each opened file | Yes, inside each opened file |
| Best for demo | Shows both skip mechanisms | Isolates row group pruning alone |

The A/B benchmark on Day 3 quantifies how much partition pruning adds on top of row group skipping.

### Partition scheme options for 5M rows

Your synthetic data will span a fixed time range (e.g. one day). How you slice it into folders is the choice:

| Scheme | Path pattern | ~# of folders (5 services, 1 day) | Pros | Cons |
|--------|-------------|-----------------------------------|------|------|
| **A. date + service** | `date=…/service=…/` | ~5 | Simple, matches "logs for service X on day Y" | Can't prune by hour |
| **B. date + hour + service** | `date=…/hour=…/service=…/` | ~120 (24×5) | Matches "last hour for service X", very observability-realistic | More files, smaller per file |
| **C. date + hour only** | `date=…/hour=…/` | ~24 | Good time-range pruning | Can't skip by service at folder level |

**Chosen: option B (`date/hour/service`)**, most realistic for observability queries ("show me payment service errors in the last hour") and enough partitions (~120) to see pruning without creating tiny files. Layout B stays flat for comparison.


## What you should understand by the end

This is a discovery project. Each tool teaches one layer of "how column search works":

| Tool | What it is | What you'll learn |
|------|-----------|-------------------|
| **Arrow** | In-memory columnar format | Data lives column-by-column in memory; zero-copy slices; why analytics engines prefer columns over rows |
| **Parquet** | On-disk columnar format | Row groups, column chunks, compression, min/max statistics; the on-disk structures that make skipping possible |
| **MinIO / object storage** | Remote file store (S3 API) | Queries read byte ranges over the network; pushdown matters even more when I/O is expensive |
| **DataFusion** | Query engine | SQL → logical plan → physical plan; predicate pushdown and column projection decide which bytes get read |

**The core insight to internalize:** A columnar DB doesn't "search faster" because of magic; it reads less data. Parquet stores per-column statistics (min/max per row group). DataFusion pushes your `WHERE` clause down to the Parquet reader, which skips entire row groups (and entire columns) that can't match. That's the whole game.


## Enhancements (all confirmed)

### 1. Parquet metadata inspection (high value, ~1 hour)

After writing files, programmatically read Parquet footer metadata: row group count, column sizes, min/max statistics per row group. Print a summary table.

**Why:** Shows you understand the on-disk layout, not just the API. You can literally point at a row group and say "this one has `status_code` min=200 max=299, so the engine skips it for `status_code >= 500`."

### 2. Partitioning A/B experiment (high value, ~2 hours)

Write the same dataset two ways:

- **Layout A:** Hive-style partitions `date=2026-09-01/hour=14/service=api/` (or by 5-min window)
- **Layout B:** Single directory, no partitioning

Run the same time-range + service filter query on both. Compare `EXPLAIN` output and latency.

**Why:** Partition pruning and row-group skipping are complementary. Tsuga-scale systems use both. This gives you a concrete story about tradeoffs (more files vs. finer skip granularity).

### 3. Selectivity ladder benchmark (high value, ~1 hour)

Run the same aggregation query with progressively tighter filters:

| Filter | Expected rows scanned |
|--------|----------------------|
| No filter | 100% |
| `status_code >= 500` (~5% of rows) | ~5% |
| `status_code = 503 AND service = 'payments'` | <1% |

Record latency and (if possible) bytes read for each.

**Why:** Demonstrates that pushdown benefit scales with selectivity, the most important property of columnar query engines.

### 4. Column projection demo (medium value, ~30 min)

Compare `SELECT *` vs. `SELECT service, latency_ms` on the same filter. Show size difference in `EXPLAIN` or measured I/O.

**Why:** Reinforces that columnar formats skip columns entirely, not just rows.

### 5. Structured Cargo workspace (medium value, ~1 hour)

```
parqit/
├── crates/
│   ├── generator/    # synthetic data → Arrow → Parquet
│   ├── inspect/      # read Parquet metadata, print stats
│   └── query/        # DataFusion CLI against MinIO
├── docker-compose.yml
├── data/             # gitignored
└── README.md
```

**Why:** Clean separation mirrors real pipeline stages. Easier to demo one piece at a time to the CEO.

### 6. One-command demo script (medium value, ~30 min)

`./scripts/demo.sh` starts MinIO, uploads data, runs 3 queries with `EXPLAIN`, and prints the benchmark table.

**Why:** Interview demos fail when you type live. A script guarantees reproducible numbers.

### 7. Richer synthetic schema (low effort, high demo value)

Add columns beyond the original five:

```
timestamp, service, status_code, latency_ms, trace_id,
http_method, route, region
```

- `service`, `http_method`, `region` → low cardinality (dictionary encoding wins)
- `route`, `trace_id` → high cardinality (compression tradeoffs)

**Why:** More realistic observability shape; lets you show dictionary encoding in Parquet metadata.

### 8. Vortex comparison (**deferred**)

Repeat the dataset in Vortex format. Compare compression ratio and query latency against Parquet.

**Status:** Skipped for now. Revisit only if Days 1–4 finish early.

### 9. CEO talk track (zero code, high value)

A short section in the README: "If I had 5 minutes, I'd show…" with 3 slides worth of narrative.

**Why:** The code proves you can build; the talk track proves you understand why.


## Architecture

```
Synthetic data generator (Rust + rand)
        │
        ▼
  Arrow RecordBatches          ← in-memory columnar (process)
        │
        ▼
  Parquet files (partitioned)  ← on-disk columnar + statistics (index)
        │
        ▼
  MinIO (S3-compatible)        ← object storage
        │
        ▼
  DataFusion SQL engine        ← pushdown + projection (query)
        │
        ▼
  Benchmarks + README + demo script
```


## Day 1: Generate data, write Parquet, inspect metadata

**Focus:** Arrow + Parquet write path. Understand what's on disk.

- [ ] Scaffold Cargo workspace (`generator`, `inspect`, `query` crates)
- [ ] Generate synthetic HTTP request logs (**5M rows**):
  - `timestamp`, `service`, `status_code`, `latency_ms`, `trace_id`, `http_method`, `route`, `region`
- [ ] Build as Arrow `RecordBatch`es, write with `parquet` crate
  - Use Snappy or Zstd compression
  - Set row group size explicitly (e.g. 100k rows); note this in README
  - Ensure statistics are written (default, but verify)
- [ ] Write **Layout A** (partitioned by date + service) and **Layout B** (flat)
- [ ] Build `inspect` tool: read footer metadata, print per-row-group column stats
- [ ] Record: raw JSON/CSV size vs. Parquet size (compression ratio)

**Discovery questions to answer today:**
- What does a row group look like in the metadata?
- Which columns got dictionary encoding?
- How big is each column chunk vs. the whole file?

**Deliverable:** `data/` folder with Parquet files + `inspect` output saved to `results/metadata.txt`.


## Day 2: Object storage + first queries

**Focus:** DataFusion + MinIO. See pushdown in action.

- [ ] `docker-compose.yml` with MinIO (console on `:9001`, API on `:9000`)
- [ ] Upload Parquet files to bucket (both layouts)
- [ ] Register as DataFusion tables via `object_store` + `ListingTable`
  - Use Hive-style partition columns for Layout A
- [ ] Run core queries:
  ```sql
  SELECT service, COUNT(*) AS errors
  FROM logs
  WHERE status_code >= 500 AND ts > '2026-09-01'
  GROUP BY service;

  SELECT route, approx_percentile_cont(latency_ms, 0.99) AS p99
  FROM logs
  WHERE service = 'api'
  GROUP BY route
  ORDER BY p99 DESC
  LIMIT 10;
  ```
- [ ] Run `EXPLAIN` and `EXPLAIN ANALYZE` on each query
- [ ] Save explain output to `results/explain/`

**Discovery questions to answer today:**
- Does `EXPLAIN` show `PruningPredicate` or row group filtering?
- What's different between Layout A and Layout B explain plans?
- Which columns appear in the physical plan's projection?

**Deliverable:** Working DataFusion queries against MinIO with saved `EXPLAIN` output.


## Day 3: Benchmarks + selectivity + projection

**Focus:** Quantify why columnar wins. This is the CEO demo core.

- [ ] Generate equivalent JSON/CSV dataset (same rows, for naive baseline)
- [ ] Benchmark harness (simple: `Instant::now()` + run N times, report median):
  1. Naive full scan (read all JSON/CSV, filter in Rust)
  2. DataFusion full scan (no WHERE)
  3. DataFusion selective filter (`status_code >= 500`)
  4. DataFusion tight filter (`status_code = 503 AND service = 'payments'`)
  5. `SELECT *` vs. `SELECT service, latency_ms` (column projection)
  6. Layout A (partitioned) vs. Layout B (flat), same query
- [ ] Record in a results table:

  | Scenario | Latency (ms) | Data read | Notes |
  |----------|-------------|-----------|-------|
  | ... | ... | ... | ... |

- [ ] Save raw numbers to `results/benchmarks.json` or markdown table

**Discovery questions to answer today:**
- At what selectivity does Parquet pull ahead of naive scan?
- How much does partition pruning add on top of row group skipping?
- What's the S3 overhead vs. local disk for the same query?

**Deliverable:** Benchmark numbers you can quote in the interview.


## Day 4: Polish, README, demo script

**Focus:** Make it presentable. Rehearse the narrative.

- [ ] Write `README.md`:
  - One-paragraph architecture
  - How columnar search works (row groups, statistics, pushdown) in your own words
  - Benchmark table
  - Partitioning tradeoffs (what you'd change at real scale)
  - "What I learned" section
- [ ] `scripts/demo.sh`, one command to run the full demo
- [ ] Clean up code, add brief comments only where non-obvious
- [ ] Label dataset as synthetic everywhere

**Deliverable:** GitHub-ready repo with README and demo script.


## Day 5: Extra buffer / deep dives

Vortex deferred. Use this day for anything that slipped, or go deeper:

- [ ] Re-run benchmarks, tighten numbers
- [ ] Add local-disk vs MinIO comparison (S3 overhead story)
- [ ] Rehearse talk track once
- [ ] (Optional) Vortex comparison if days 1–4 finished early


## Day 6 (Sun Sep 28): CEO rehearsal

- [ ] Run `./scripts/demo.sh` end-to-end twice and fix anything flaky
- [ ] Prepare 5-minute talk track (see below)
- [ ] Anticipate 3 follow-up questions (see below)
- [ ] Push final repo, have URL ready


## What to show the CEO (5-minute talk track)

**Opening (30 sec):**
> "After the HR round I wanted hands-on time with the stack your query team uses, so I built a small observability pipeline: synthetic logs → Arrow → Parquet → MinIO → DataFusion. It's synthetic data, but the query patterns mirror real log analytics."

**Demo (3 min):**
1. Show `inspect` output: row groups, column stats, dictionary encoding
2. Run selective query, show `EXPLAIN`: point at row group pruning
3. Show benchmark table: naive scan vs. pushdown at different selectivities

**Closing (1 min):**
> "The main takeaway: columnar engines win by reading less. Row group statistics and partition layout determine how much. At real scale I'd add streaming ingestion, a smarter partitioning strategy keyed on query patterns, and metadata indexing for high-cardinality columns like trace_id."

**Numbers to have memorized:**
- Compression ratio (raw → Parquet)
- Naive scan vs. pushdown latency at ~5% selectivity
- Partitioned vs. flat layout latency delta


## Follow-up questions to prepare for

1. **"What happens if Parquet statistics are wrong or missing?"**
   → Engine falls back to reading the row group anyway; correct results, no pruning benefit. Statistics are best-effort hints.

2. **"Why partition by time vs. service?"**
   → Time matches the most common query pattern (recent logs). Service partitioning helps when queries are scoped to one service. Too many partitions → small files → metadata overhead.

3. **"How would this change at 100x scale?"**
   → Streaming ingestion (Kafka → batch writer), compaction of small files, distributed query coordinator, column-specific indexes for high-cardinality lookups (trace_id), possibly hot/cold tiering in object storage.

4. **"Why Arrow as an intermediate format?"**
   → Zero-copy sharing between process and query stages; canonical columnar layout that Parquet, DataFusion, and Flight all speak.

5. **"What's the difference between predicate pushdown and partition pruning?"**
   → Partition pruning skips whole files/directories based on partition column values. Predicate pushdown skips row groups within a file based on column statistics. Both reduce I/O; they stack.


## Out of scope (don't build)

- Real-time streaming ingestion
- Custom indexing beyond Parquet's built-in statistics
- Distributed query execution
- Production error handling, auth, monitoring
- Anything that delays the core demo past Saturday


## Decision log

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Dataset size | **5M rows** | Enough to see latency gaps; generates in minutes |
| Schema | **8 columns** | Realistic observability shape; dictionary encoding demo |
| Enhancements | **All kept** | Maximize interview prep value |
| Vortex | **Deferred** | Focus on core stack first |
| Row group size | TBD (suggest 100k) | Smaller = finer pruning; larger = better compression |
| Partition scheme | **`date/hour/service`** (option B) | ~120 folders; matches observability query patterns |
| Compression codec | TBD (suggest Snappy) | Fast decode; good enough for demo |


## v1 → v2 changes summary

| Area | v1 | v2 |
|------|----|----|
| Timeline | 2–3 days | 6 days with buffer |
| Learning objectives | Implicit | Explicit per-tool table |
| Parquet inspection | Not included | Day 1 deliverable |
| Partitioning | Mentioned | A/B experiment with benchmarks |
| Benchmarks | 2-way (naive vs. pushdown) | 6-scenario ladder + projection |
| Schema | 5 columns | 8 columns ( richer observability shape) |
| Project structure | Single binary | Cargo workspace |
| Demo | Manual | Scripted + talk track |
| CEO prep | One-liner | 5-min talk track + Q&A prep |
