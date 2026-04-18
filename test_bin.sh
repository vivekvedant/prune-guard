#!/usr/bin/env bash
set -euo pipefail

# Keep growing Docker storage by creating unique image layers, stopped
# containers, and non-empty volumes on every iteration.
IMAGE_PREFIX="${PG_STRESS_IMAGE_PREFIX:-pg-storage-grow}"
CONTAINER_PREFIX="${PG_STRESS_CONTAINER_PREFIX:-pg-storage-grow-c}"
VOLUME_PREFIX="${PG_STRESS_VOLUME_PREFIX:-pg-storage-grow-v}"
CHUNK_MB="${PG_STRESS_CHUNK_MB:-64}"
SLEEP_SECONDS="${PG_STRESS_SLEEP_SECONDS:-1}"
MAX_ITERATIONS="${PG_STRESS_MAX_ITERATIONS:-0}" # 0 means run forever.
WORK_DIR="${PG_STRESS_WORK_DIR:-/tmp/prune-guard-storage-grow}"

require_positive_int() {
  local value="$1"
  local name="$2"
  if [[ ! "$value" =~ ^[0-9]+$ ]]; then
    echo "invalid ${name}: ${value}"
    exit 1
  fi
}

command -v docker >/dev/null 2>&1 || { echo "docker command not found"; exit 1; }
docker info >/dev/null 2>&1 || { echo "docker daemon is not reachable"; exit 1; }

require_positive_int "$CHUNK_MB" "PG_STRESS_CHUNK_MB"
require_positive_int "$SLEEP_SECONDS" "PG_STRESS_SLEEP_SECONDS"
require_positive_int "$MAX_ITERATIONS" "PG_STRESS_MAX_ITERATIONS"

mkdir -p "$WORK_DIR"
docker pull alpine:latest >/dev/null

iteration=0
while :; do
  iteration=$((iteration + 1))
  if (( MAX_ITERATIONS > 0 && iteration > MAX_ITERATIONS )); then
    echo "Reached PG_STRESS_MAX_ITERATIONS=${MAX_ITERATIONS}; exiting."
    break
  fi

  stamp="$(date +%s)-${iteration}"
  tag="${IMAGE_PREFIX}:${stamp}"
  container_name="${CONTAINER_PREFIX}-${stamp}"
  volume_name="${VOLUME_PREFIX}-${stamp}"
  build_dir="$(mktemp -d "${WORK_DIR}/build-${stamp}-XXXX")"
  payload_file="${build_dir}/payload.bin"

  # Random content makes every layer unique so storage keeps increasing.
  dd if=/dev/urandom of="${payload_file}" bs=1M count="${CHUNK_MB}" >/dev/null 2>&1

  cat > "${build_dir}/Dockerfile" <<EOF
FROM alpine:latest
COPY payload.bin /opt/payload.bin
EOF

  docker build --no-cache -t "${tag}" "${build_dir}" >/dev/null
  docker create --name "${container_name}" "${tag}" >/dev/null
  docker volume create "${volume_name}" >/dev/null
  docker run --rm -v "${volume_name}:/data" alpine:latest sh -c \
    "dd if=/dev/urandom of=/data/payload-${stamp}.bin bs=1M count=${CHUNK_MB} >/dev/null 2>&1"

  rm -rf "${build_dir}"

  echo "Iteration ${iteration}: image=${tag} container=${container_name} volume=${volume_name}"
  docker system df
  sleep "${SLEEP_SECONDS}"
done
