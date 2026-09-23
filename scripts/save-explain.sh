#!/usr/bin/env bash
# Save Day 2 EXPLAIN outputs for hive vs flat comparison.
set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p results/explain

HIVE_FILTER="SELECT COUNT(*) AS n FROM logs WHERE date = '2026-09-01' AND hour = '14' AND service = 'payments' AND status_code >= 500"
FLAT_FILTER="SELECT COUNT(*) AS n FROM logs_flat WHERE service = 'payments' AND status_code >= 500"

echo "Saving EXPLAIN plans to results/explain/ ..."

cargo run -q -p query -- --explain --sql "$HIVE_FILTER" \
  > results/explain/hive_explain.txt 2>&1

cargo run -q -p query -- --layout flat --path layout_b --table logs_flat --explain --sql "$FLAT_FILTER" \
  > results/explain/flat_explain.txt 2>&1

cargo run -q -p query -- --explain-analyze --sql "$HIVE_FILTER" \
  > results/explain/hive_explain_analyze.txt 2>&1

cargo run -q -p query -- --layout flat --path layout_b --table logs_flat --explain-analyze --sql "$FLAT_FILTER" \
  > results/explain/flat_explain_analyze.txt 2>&1

echo "Done:"
ls -1 results/explain/
