#!/usr/bin/env bash
# Compare local parquet file counts vs MinIO.
set -euo pipefail

cd "$(dirname "$0")/.."

BUCKET="${BUCKET:-logs}"
MC="docker run --rm --network host --entrypoint sh quay.io/minio/mc:RELEASE.2025-08-13T08-35-41Z -c"

count_local() {
  find "data/$1" -name '*.parquet' 2>/dev/null | wc -l | tr -d ' '
}

count_minio() {
  $MC "mc alias set local http://127.0.0.1:9000 minioadmin minioadmin >/dev/null \
    && mc find local/${BUCKET}/$1 --name '*.parquet' | wc -l" | tr -d ' '
}

for layout in raw compacted; do
  if [[ -d "data/${layout}" ]]; then
    local_n=$(count_local "$layout")
    minio_n=$(count_minio "$layout" || echo "?")
    echo "${layout}: local=${local_n}  minio=${minio_n}"
  fi
done

# Stray files at the prefix root break DataFusion hive partitioning.
strays=$($MC "mc alias set local http://127.0.0.1:9000 minioadmin minioadmin >/dev/null \
  && mc ls local/${BUCKET}/raw/*.parquet 2>/dev/null" | wc -l | tr -d ' ')
if [[ "${strays:-0}" -gt 0 ]]; then
  echo "WARNING: remove stray parquet under raw/ (e.g. test.parquet) before benchmarking"
fi
