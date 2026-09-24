#!/usr/bin/env bash
# Upload v3 raw and/or compacted layouts to MinIO (day-by-day, resumable).
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="${1:-both}"
BUCKET="${BUCKET:-logs}"
MC_IMAGE="${MC_IMAGE:-quay.io/minio/mc:RELEASE.2025-08-13T08-35-41Z}"
RETRIES="${RETRIES:-3}"
# Parallel mc workers cause spurious "Insufficient permissions" against MinIO.
MAX_WORKERS="${MAX_WORKERS:-1}"

mc_run() {
  docker run --rm --network host \
    -v "$(pwd)/data:/data:ro" \
    --entrypoint sh "$MC_IMAGE" \
    -c "$1"
}

setup_mc() {
  mc_run "mc alias set local http://127.0.0.1:9000 minioadmin minioadmin \
    && mc mb --ignore-existing local/${BUCKET}"
}

count_local_day() {
  find "data/$1/$2" -name '*.parquet' 2>/dev/null | wc -l | tr -d ' '
}

count_remote_day() {
  local src_root="$1"
  local dst_root="$2"
  local day="$3"
  mc_run "mc alias set local http://127.0.0.1:9000 minioadmin minioadmin >/dev/null \
    && mc find local/${BUCKET}/${dst_root}/${day} --name '*.parquet'" 2>/dev/null | wc -l | tr -d ' '
}

upload_day() {
  local src_root="$1"
  local dst_root="$2"
  local day="$3"
  local local_n remote_n attempt=1

  local_n=$(count_local_day "$src_root" "$day")
  remote_n=$(count_remote_day "$src_root" "$dst_root" "$day")

  if [[ "$local_n" -eq 0 ]]; then
    echo "  ${day}: skip (no local files)"
    return 0
  fi

  if [[ "$local_n" -eq "$remote_n" ]]; then
    echo "  ${day}: skip (${local_n} files already on MinIO)"
    return 0
  fi

  echo "  ${day}: uploading ${local_n} files (${remote_n} already on MinIO)..."

  while (( attempt <= RETRIES )); do
    if mc_run "mc cp --recursive --max-workers ${MAX_WORKERS} \
        /data/${src_root}/${day}/ local/${BUCKET}/${dst_root}/${day}/"; then
      remote_n=$(count_remote_day "$src_root" "$dst_root" "$day")
      if [[ "$local_n" -eq "$remote_n" ]]; then
        echo "  ${day}: done (${remote_n} files)"
        return 0
      fi
      echo "  ${day}: count mismatch after upload (local=${local_n} remote=${remote_n})" >&2
    fi
    echo "  ${day}: attempt ${attempt} failed, retrying..." >&2
    (( attempt++ )) || true
    sleep 2
  done

  echo "ERROR: gave up on ${day} after ${RETRIES} attempts" >&2
  return 1
}

upload_layout() {
  local src="$1"
  local dst="$2"

  if [[ ! -d "data/${src}" ]]; then
    echo "Skip ${src}: data/${src} not found"
    return 0
  fi

  echo "Uploading data/${src} → s3://${BUCKET}/${dst}/"
  for day_dir in data/"${src}"/date=*; do
    [[ -d "$day_dir" ]] || continue
    upload_day "$src" "$dst" "$(basename "$day_dir")"
  done
}

setup_mc

case "$TARGET" in
  raw)       upload_layout raw raw ;;
  compacted) upload_layout compacted compacted ;;
  both)
    upload_layout raw raw
    upload_layout compacted compacted
    ;;
  *)
    echo "Usage: $0 [raw|compacted|both]"
    exit 1
    ;;
esac

echo "Done."
