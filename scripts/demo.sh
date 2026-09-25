#!/usr/bin/env bash
# End-to-end demo: synthetic logs → Parquet → MinIO → DataFusion
#
# Usage:
#   ./scripts/demo.sh          # auto: v3 if data/manifest.json + compacted exist, else v2
#   ./scripts/demo.sh v3
#   ./scripts/demo.sh v2
#   DEMO=v3 make demo
set -euo pipefail

cd "$(dirname "$0")/.."

MODE="${DEMO:-${1:-auto}}"
BUCKET="${BUCKET:-logs}"
MC_IMAGE="${MC_IMAGE:-quay.io/minio/mc:RELEASE.2025-08-13T08-35-41Z}"

step() { echo; echo "── $1 ──"; echo; }

mc_run() {
  docker run --rm --network host \
    -v "$(pwd)/data:/data:ro" \
    --entrypoint sh "$MC_IMAGE" \
    -c "$1"
}

count_minio_prefix() {
  mc_run "mc alias set local http://127.0.0.1:9000 minioadmin minioadmin >/dev/null \
    && mc find local/${BUCKET}/$1 --name '*.parquet'" 2>/dev/null | wc -l | tr -d ' '
}

ensure_minio() {
  if ! docker compose ps --status running 2>/dev/null | grep -q minio; then
    echo "Starting MinIO..."
    docker compose up -d
    sleep 2
  fi
  curl -sf http://localhost:9000/minio/health/live >/dev/null \
    || { echo "MinIO not reachable on :9000"; exit 1; }
  echo "MinIO: OK"
}

detect_mode() {
  if [[ "$MODE" == "v2" || "$MODE" == "v3" ]]; then
    echo "$MODE"
    return
  fi
  if [[ -f data/manifest.json && -d data/compacted ]]; then
    echo "v3"
  elif [[ -f data/layout_b/part-000.parquet ]]; then
    echo "v2"
  else
    echo "none"
  fi
}

sample_compacted_file() {
  find data/compacted -path '*hour=14*service=payments*' -name '*.parquet' 2>/dev/null | head -1 \
    || find data/compacted -name '*.parquet' 2>/dev/null | head -1
}

upload_v2_if_needed() {
  local hive_n
  hive_n=$(count_minio_prefix layout_a || echo 0)
  if [[ "$hive_n" -ge 100 ]]; then
    echo "MinIO layout_a: ${hive_n} files (skip upload)"
    return
  fi
  echo "Uploading v2 layouts to s3://${BUCKET}/ ..."
  mc_run "mc alias set local http://127.0.0.1:9000 minioadmin minioadmin \
    && mc mb --ignore-existing local/${BUCKET} \
    && mc cp --recursive /data/layout_a/ local/${BUCKET}/layout_a/ \
    && mc cp --recursive /data/layout_b/ local/${BUCKET}/layout_b/"
}

upload_v3_compacted_if_needed() {
  local local_n minio_n
  local_n=$(find data/compacted -name '*.parquet' 2>/dev/null | wc -l | tr -d ' ')
  minio_n=$(count_minio_prefix compacted || echo 0)

  if [[ "$local_n" -eq 0 ]]; then
    echo "No files under data/compacted/. Run: make compact" >&2
    exit 1
  fi

  if [[ "$minio_n" -ge "$local_n" && "$local_n" -gt 0 ]]; then
    if [[ "$minio_n" -ne "$local_n" ]]; then
      echo "WARNING: MinIO compacted=${minio_n}, local=${local_n} (stale objects? queries still work)"
    else
      echo "MinIO compacted: ${minio_n} files (skip upload)"
    fi
    return
  fi

  echo "Uploading compacted layout (${local_n} local, ${minio_n} on MinIO)..."
  ./scripts/upload-v3.sh compacted
}

run_demo_v2() {
  echo "=== parqit demo (v2 — 5M rows, layout A/B) ==="

  step "1. Check prerequisites"
  ensure_minio

  if [[ ! -f data/layout_b/part-000.parquet ]]; then
    echo "No v2 data found. Generating 5M rows (~2 min)..."
    make generate
  fi
  upload_v2_if_needed
  echo "Local Parquet: OK"

  step "2. Parquet metadata (row groups, stats, encodings)"
  cargo run -q -p inspect -- \
    "data/layout_a/date=2026-09-01/hour=14/service=payments/part-000.parquet" \
    | head -20

  step "3. DataFusion EXPLAIN — partition pruning + pushdown"
  cargo run -q -p query -- --explain --sql \
    "SELECT COUNT(*) AS errors FROM logs WHERE status_code >= 500 AND date = '2026-09-01' AND hour = '14' AND service = 'payments'"

  step "4. Live query result"
  cargo run -q -p query -- --sql \
    "SELECT service, COUNT(*) AS errors FROM logs WHERE status_code >= 500 AND date = '2026-09-01' AND hour = '14' AND service = 'payments' GROUP BY service"

  step "5. Benchmark summary"
  if [[ -f results/benchmarks.md ]]; then
    sed -n '/^## Summary/,$p' results/benchmarks.md
  else
    echo "No results/benchmarks.md — run: make bench-step1 && make bench-step2"
  fi

  echo
  echo "=== Demo complete (v2) ==="
  echo "Full benchmarks: results/benchmarks.md"
}

run_demo_v3() {
  echo "=== parqit demo (v3 — compacted layout, incident drill) ==="

  step "1. Check prerequisites"
  ensure_minio

  if [[ ! -f data/manifest.json ]]; then
    echo "No data/manifest.json. Generate v3 data first, e.g.:"
    echo "  make generate-v3-smoke && make compact"
    echo "  make generate-v3-standard && make compact   # full 200M run"
    exit 1
  fi

  if [[ ! -d data/compacted ]]; then
    echo "No data/compacted/. Merge micro-batches first:"
    echo "  make compact"
    exit 1
  fi

  upload_v3_compacted_if_needed
  ./scripts/verify-minio.sh 2>/dev/null | grep -E '^(raw|compacted):' || true

  local sample
  sample=$(sample_compacted_file)
  if [[ -z "$sample" ]]; then
    echo "No parquet files under data/compacted/" >&2
    exit 1
  fi

  step "2. Parquet metadata (compacted partition file)"
  cargo run -q -p inspect -- "$sample" | head -20

  step "3. DataFusion EXPLAIN — incident workload (15m 5xx window)"
  cargo run -q -p query -- \
    --path compacted \
    --workload incident \
    --manifest data/manifest.json \
    --explain

  step "4. Live query — incident workload (first rows)"
  set +o pipefail
  cargo run -q -p query -- \
    --path compacted \
    --workload incident \
    --manifest data/manifest.json \
    | head -25
  set -o pipefail

  step "5. Benchmark summary (v3)"
  if [[ -f results/benchmarks_v3.md ]]; then
    echo "Single-threaded (median of 3):"
    sed -n '/^## Compacted/,/^## Raw vs compacted/p' results/benchmarks_v3.md | head -n -1
    if grep -q 'Step 4' results/benchmarks_v3.md; then
      echo
      echo "Concurrent load (Step 4, overall p50/p95/p99):"
      sed -n '/^### Raw vs compacted (overall p50)/,/^_/p' results/benchmarks_v3.md
    fi
  else
    echo "No results/benchmarks_v3.md — run: make bench-v3-all"
  fi

  echo
  echo "=== Demo complete (v3) ==="
  echo "Full benchmarks: results/benchmarks_v3.md"
  echo "Try: cargo run -p query -- --path compacted --workload dashboard"
}

RESOLVED=$(detect_mode)

case "$RESOLVED" in
  v2) run_demo_v2 ;;
  v3) run_demo_v3 ;;
  none)
    echo "No dataset found. Pick one:"
    echo "  v2: make generate && ./scripts/demo.sh v2"
    echo "  v3: make generate-v3-smoke && make compact && ./scripts/demo.sh v3"
    exit 1
    ;;
  *)
    echo "Unknown mode: $MODE (use v2, v3, or auto)"
    exit 1
    ;;
esac
