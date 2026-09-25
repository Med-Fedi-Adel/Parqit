# parqit v3: production-scale stress test

v2 proved pushdown on 5M rows and 120 files. That's a toy. Real observability pipelines deal with days of retention, thousands of small ingest files, skewed traffic, and query mixes that punish bad layout choices.

v3 keeps the same stack (Rust, Arrow, Parquet, MinIO, DataFusion) but changes the *shape* of the data and the *benchmark suite* to match what breaks in production.

---

## What v2 doesn't exercise

| Gap | v2 today | Typical production |
|-----|----------|-------------------|
| Volume | 5M rows (~240 MB) | 100M–1B+ rows/day tiered by retention |
| Time range | 1 day (`2026-09-01`) | 7–30 day hot window, longer cold |
| Ingest pattern | 1 file per partition | Many small files per hour (micro-batch flush) |
| Services | 5, uniform share | 20–50 services, heavy skew |
| Error distribution | flat ~5% 5xx | baseline + injectable incident windows |
| Queries | mostly counts on known partitions | dashboards, trace lookup, cross-day scans, concurrent load |
| File lifecycle | write once | ingest → compact → query (possibly different layouts) |
| Stress signals | median latency only | p95/p99, bytes read, files opened, RAM, list amplification |

---

## Production patterns to mimic

### 1. Micro-batch ingest (file sprawl)

Agents (Vector, Fluent Bit, OTel collector) flush to object storage every 1–15 minutes. You get *lots* of small Parquet files under the same `date/hour/service` prefix, not one neat `part-000.parquet`.

**v3 change:** generator writes **N files per partition** with realistic row counts per file (1k–50k rows pre-compaction).

### 2. Compaction

A background job merges small files into fewer, larger ones (target 64–256 MB). Queries hit compacted data most of the time; raw ingest layout is what you debug when things are slow.

**v3 change:** new `compact` step merges files within a partition. Benchmark both **raw** and **compacted** layouts.

### 3. Retention window

Dashboards scan "last 24h". Weekly reports scan 7 days. Incidents drill into 15 minutes.

**v3 change:** multi-day dataset (default **7 days**). Queries explicitly span 1h, 24h, and 7d.

### 4. Skew and incidents

`payments` and `api` dominate volume. Errors cluster in time during outages.

**v3 change:** configurable service weights (Zipf-like). Optional **incident profile** that spikes 5xx on a service for 1–2 hours on day 3.

### 5. Query mix (the part that actually hurts)

Production isn't one `COUNT(*)` with perfect partition keys. Typical mix:

| Workload | Example | What it stress-tests |
|----------|---------|---------------------|
| **Dashboard** | error rate by service, last 1h | partition + column pruning |
| **Incident** | all 5xx in last 15m, all services | recent time filter, many partitions touched |
| **Scoped day** | payments errors, full day | single service, 24 hourly files |
| **Cross-day report** | p99 latency by route, 7 days | wide time scan, aggregation memory |
| **Trace lookup** | `WHERE trace_id = '…'` | high-cardinality point query, no partition help |
| **Full scan** | `COUNT(*)` over retention | object listing + metadata overhead |
| **Concurrent** | 8 clients running the mix above | CPU, MinIO connection pool, cache effects |

**v3 change:** replace the 8-scenario bench with this catalog. Add concurrency mode.

---

## Scale tiers

Pick one default; support all via flags.

| Tier | Rows | Days | Services | Raw files (est.) | Parquet (est.) | Use case |
|------|------|------|----------|------------------|----------------|----------|
| **smoke** | 10M | 1 | 5 | ~500 | ~500 MB | CI, laptop dev |
| **standard** | 200M | 7 | 20 | ~15k–40k | ~10 GB | **v3 default** |
| **stress** | 1B | 14 | 50 | ~200k+ | ~50 GB | disk/RAM limit test |

Row size stays roughly the same as v2 (~48 B compressed). **Standard** is the sweet spot: big enough to feel object-store pain, small enough to generate overnight on a decent machine.

Disk budget: plan for **3× Parquet size** (raw + compacted + JSONL export optional).

---

## Layouts in v3

Keep A/B comparison spirit, rename for clarity:

| Layout | Path | Role |
|--------|------|------|
| **raw** | `date/hour/service/part-{batch}.parquet` | Simulates live ingest. Many small files. |
| **compacted** | `date/hour/service/part-{NNN}.parquet` | Post-compaction. Fewer, larger files. Same data. |
| **flat** | `layout_flat/part-{chunk}.parquet` | Control. Sharded into ~128 MB chunks (not one giant file at 200M+ rows). |

Partition scheme stays **`date/hour/service`** for raw/compacted. At 200M / 7d / 20svc that's 3,360 partition-hours before file multiplication.

Flat layout at billion-row scale is optional; mainly keep it for row-group pruning experiments on a bounded sample.

---

## Schema changes (minimal)

Keep v2 columns for continuity. Add only what production logs usually carry and what breaks encodings:

| Column | Type | Notes |
|--------|------|-------|
| `timestamp` | μs UTC | unchanged |
| `service` | string | partition key (raw/compacted) |
| `status_code` | i32 | unchanged |
| `latency_ms` | i32 | unchanged |
| `trace_id` | string | point-lookup workload |
| `http_method` | string | dictionary-friendly |
| `route` | string | high cardinality, long tail |
| `region` | string | unchanged |
| **`pod`** | string | **new**, ~200 values, simulates K8s pod name |
| **`environment`** | string | **new**, `prod` / `staging` (99% prod) |

No nested JSON blobs in v3. Keep the generator simple; realism comes from volume and file layout, not schema explosion.

---

## Crate changes

### `generator`

- `--days 7`, `--services 20`, `--tier standard|smoke|stress`
- `--ingest-interval-min 15`: files flushed per partition per interval
- `--service-skew 1.2`: Zipf exponent (higher = more skew toward hot services)
- `--incident day=3,hour=14-16,service=payments,error-rate=0.25`: optional spike
- Write **raw** layout only; compaction is separate
- Progress + ETA; parallel partition writers (rayon) for standard/stress tiers
- Stats output: row counts, file counts, bytes, incident rows

### `compact` (new crate or `generator compact` subcommand)

- Input: raw layout prefix
- Output: compacted layout prefix
- Merge all files in each `date/hour/service` directory
- Target max file size ~128 MB (split if merged result exceeds)
- Preserve row group size (100k) and statistics
- Report: files before/after, bytes before/after, wall time

### `inspect`

- `--summary`: one line per partition (file count, total rows, total bytes)
- `--worst-partitions 10`: partitions with most files (the ones that hurt list ops)
- Helps explain slow queries without opening every footer

### `query`

- Predefined workload SQL files under `queries/` (versioned, readable)
- `--workload dashboard|incident|trace|…`
- `--concurrency N` for repeated runs
- Optional: print `files_scanned` if we can extract from plan stats

### `benchmark`

- **step3-raw:** workloads against raw layout
- **step3-compacted:** same workloads against compacted
- **step3-concurrent:** 8-thread mix, report p50/p95/p99
- **step3-compare:** side-by-side raw vs compacted table
- Capture where possible: `EXPLAIN ANALYZE` bytes, listing time (wall clock delta for `COUNT(*)` vs tight filter)

---

## Workload SQL (draft)

```sql
-- dashboard: last hour, all services
SELECT service, COUNT(*) FILTER (WHERE status_code >= 500) AS errors
FROM logs
WHERE date = '2026-09-07' AND hour = '23'
GROUP BY service;

-- incident: 15-minute window, all services (narrow hour + timestamp filter)
SELECT service, status_code, COUNT(*) AS n
FROM logs
WHERE date = '2026-09-07' AND hour = '14'
  AND timestamp >= '2026-09-07T14:30:00Z'
  AND timestamp <  '2026-09-07T14:45:00Z'
  AND status_code >= 500
GROUP BY service, status_code;

-- trace lookup (worst case for columnar without index)
SELECT timestamp, service, route, status_code, latency_ms
FROM logs
WHERE trace_id = '{pick from seed manifest}';

-- cross-day report
SELECT route, approx_percentile_cont(latency_ms, 0.99) AS p99
FROM logs
WHERE service = 'api'
  AND date BETWEEN '2026-09-01' AND '2026-09-07'
GROUP BY route
ORDER BY p99 DESC
LIMIT 20;
```

Store picked `trace_id` values in `data/manifest.json` at generate time so lookups always hit.

---

## Implementation phases

### Phase 1: Scale the generator (foundation)

**Goal:** 200M rows, 7 days, skew, multi-file raw layout.

- [ ] Multi-day partition keys
- [ ] Service list expanded to 20 with Zipf weights
- [ ] Micro-batch file writer (multiple parquets per partition)
- [ ] Incident injection flag
- [ ] `manifest.json` with sample trace_ids per day
- [ ] Makefile: `make generate-standard`, `make generate-smoke`
- [ ] Update `data-stats` for file counts at scale

**Done when:** `du -sh data/raw` ≈ 10 GB, file count in the thousands, inspect summary runs in <30s.

### Phase 2: Compaction

**Goal:** compacted layout that queries should prefer.

- [ ] `compact` command merges per-partition files
- [ ] Verify row counts match raw (checksum query)
- [ ] Makefile: `make compact`
- [ ] Document size reduction (metadata overhead, fewer footers)

**Done when:** file count drops 80–95%, total bytes similar ±5%.

### Phase 3: Workload benchmark suite

**Goal:** production query mix, raw vs compacted numbers.

- [ ] `queries/*.sql` + workload runner
- [ ] bench step3-raw, step3-compacted
- [ ] Results appended to `results/benchmarks_v3.md`
- [ ] Each row: workload, layout, latency, files (if available), notes

**Done when:** can show "incident query 12× slower on raw than compacted" or "trace lookup equally bad on both" with real numbers.

### Phase 4: Concurrency and limits

**Goal:** stress the system, not just one query.

- [ ] Concurrent benchmark (8 workers, mixed workload)
- [ ] p50/p95/p99 reporting
- [ ] Optional: docker memory limit on MinIO to observe pressure
- [ ] Optional: `stress` tier at 1B if disk allows

**Done when:** have a concurrency row in benchmarks and can point at where latency diverges.

### Phase 5: Docs and demo

- [ ] README section "v3 vs v2"
- [ ] `scripts/demo.sh` runs compacted path + one incident query
- [ ] Fold key v3 numbers into README (replace or supplement v2 table)

---

## Metrics table (target output)

```markdown
| Workload | Layout | p50 ms | p95 ms | Files opened | Notes |
|----------|--------|-------:|-------:|-------------:|-------|
| dashboard 1h | raw | | | | |
| dashboard 1h | compacted | | | | |
| incident 15m | raw | | | | |
| trace lookup | compacted | | | | always bad without index |
| COUNT(*) 7d | raw | | | | list-bound |
| concurrent mix | compacted | | | | 8 workers |
```

Fill this in Phase 3–4. The interesting story is usually **raw vs compacted on list-heavy queries**, not another tight-partition hero number.

---

## Makefile targets (planned)

```
make generate-smoke          # 10M, 1 day, fast
make generate-standard       # 200M, 7 days (v3 default)
make generate-stress         # 1B, 14 days (explicit opt-in)

make compact                 # raw → compacted
make inspect-summary         # partition/file overview

make bench-v3-raw
make bench-v3-compacted
make bench-v3-concurrent
make bench-v3-all            # full suite → results/benchmarks_v3.md

make upload-raw / upload-compacted   # MinIO sync scripts
```

---

## Decisions to lock before coding

| # | Question | Recommendation | Alternative |
|---|----------|----------------|-------------|
| 1 | Default tier | **standard (200M / 7d)** | smoke only if laptop disk tight |
| 2 | Ingest interval | **15 min** (4 files/hour/partition) | 5 min = more files, harder on MinIO |
| 3 | Compaction target size | **128 MB** | 64 MB if RAM-constrained |
| 4 | Keep flat layout at standard tier | **shard into ~128 MB parts**, cap total rows for flat bench | drop flat at 200M+ |
| 5 | New columns | **pod + environment** | schema unchanged, skew-only |
| 6 | Trace lookup expectation | benchmark it, document it's slow | skip (less honest) |
| 7 | MinIO upload | script with `mc mirror`, parallel | manual (painful at 15k files) |

---

## Out of scope (still)

- Kafka / real streaming pipeline
- Distributed DataFusion cluster
- Secondary indexes for `trace_id`
- Parquet column indexes / bloom filters (mention in results, don't build)
- Cloud S3 (MinIO is enough; optional note on list costs)
- Vortex

---

## Success criteria

v3 is done when you can:

1. Generate **standard** tier locally and upload to MinIO without hand-holding.
2. Show **raw vs compacted** latency on the same incident query with a clear gap.
3. Show **trace lookup** timing and explain why partition layout doesn't help.
4. Show **concurrent** p95 degrading vs single-threaded (even modestly).
5. Point at **inspect-summary** output and name the partitions that would hurt a production SRE query.

---

## Suggested order of work

If time-boxed, cut from the bottom up:

1. **Must have:** Phase 1 + Phase 3 on raw layout only (scale + workloads)
2. **Should have:** Phase 2 (compaction A/B is the production story)
3. **Nice to have:** Phase 4 concurrency, stress tier

Start Phase 1 with smoke tier to validate multi-file + multi-day logic, then kick off `generate-standard` overnight.

---

See [tech_spec.md](tech_spec.md) for v2 history and partitioning background.
