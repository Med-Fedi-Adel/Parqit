#!/usr/bin/env bash
# End-to-end demo: synthetic logs → Parquet → MinIO → DataFusion
set -euo pipefail

cd "$(dirname "$0")/.."

echo "=== parqit demo (synthetic dataset) ==="
echo

step() { echo; echo "── $1 ──"; echo; }

# ── Prerequisites ──────────────────────────────────────────────
step "1. Check prerequisites"

if ! docker compose ps --status running 2>/dev/null | grep -q minio; then
  echo "Starting MinIO..."
  docker compose up -d
  sleep 2
fi
curl -sf http://localhost:9000/minio/health/live >/dev/null \
  || { echo "MinIO not reachable on :9000"; exit 1; }
echo "MinIO: OK"

if [[ ! -f data/layout_b/part-000.parquet ]]; then
  echo "No local data found. Generating 5M rows (this takes ~2 min)..."
  make generate
fi
echo "Local Parquet: OK"

# ── Inspect metadata ───────────────────────────────────────────
step "2. Parquet metadata (row groups, stats, encodings)"
cargo run -q -p inspect -- \
  "data/layout_a/date=2026-09-01/hour=14/service=payments/part-000.parquet" \
  | head -20

# ── EXPLAIN pushdown ───────────────────────────────────────────
step "3. DataFusion EXPLAIN — partition pruning + pushdown"
cargo run -q -p query -- --explain --sql \
  "SELECT COUNT(*) AS errors FROM logs WHERE status_code >= 500 AND date = '2026-09-01' AND hour = '14' AND service = 'payments'"

# ── Live query ─────────────────────────────────────────────────
step "4. Live query result"
cargo run -q -p query -- --sql \
  "SELECT service, COUNT(*) AS errors FROM logs WHERE status_code >= 500 AND date = '2026-09-01' AND hour = '14' AND service = 'payments' GROUP BY service"

# ── Benchmarks ─────────────────────────────────────────────────
step "5. Benchmark summary"
if [[ -f results/benchmarks.md ]]; then
  sed -n '/^## Summary/,$p' results/benchmarks.md
else
  echo "No results/benchmarks.md — run: make bench-step1 && make bench-step2"
fi

echo
echo "=== Demo complete ==="
echo "Full benchmarks: results/benchmarks.md"
echo "Full explain plans: results/explain/ (run: make query-save-explains)"
