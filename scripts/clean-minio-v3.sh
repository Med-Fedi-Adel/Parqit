#!/usr/bin/env bash
# Remove v3 raw and/or compacted prefixes from MinIO.
# Use when file counts drift (stale objects after re-compaction) or before a fresh upload.
#
# Usage:
#   ./scripts/clean-minio-v3.sh           # both raw + compacted
#   ./scripts/clean-minio-v3.sh raw
#   ./scripts/clean-minio-v3.sh compacted
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="${1:-both}"
BUCKET="${BUCKET:-logs}"
MC_IMAGE="${MC_IMAGE:-quay.io/minio/mc:RELEASE.2025-08-13T08-35-41Z}"

mc_run() {
  docker run --rm --network host \
    --entrypoint sh "$MC_IMAGE" \
    -c "$1"
}

remove_prefix() {
  local prefix="$1"
  echo "Removing s3://${BUCKET}/${prefix}/ ..."
  mc_run "mc alias set local http://127.0.0.1:9000 minioadmin minioadmin >/dev/null \
    && mc rm --recursive --force local/${BUCKET}/${prefix}/" \
    || echo "  (prefix empty or already gone)"
}

curl -sf http://localhost:9000/minio/health/live >/dev/null \
  || { echo "MinIO not reachable on :9000 — run: make minio-up"; exit 1; }

case "$TARGET" in
  raw)       remove_prefix raw ;;
  compacted) remove_prefix compacted ;;
  both)
    remove_prefix raw
    remove_prefix compacted
    ;;
  *)
    echo "Usage: $0 [raw|compacted|both]"
    exit 1
    ;;
esac

echo "Done. Re-upload with: make upload-v3"
