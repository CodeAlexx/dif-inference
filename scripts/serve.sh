#!/usr/bin/env bash
# Launch the diffusion-compiler-ui server (forked serenity-server) with the
# compiler-backed worker. Port 7811 by default so it never collides with the
# mojodiffusion server on 7801.
#   scripts/serve.sh [--port N] [extra serenity-server args]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export SERENITY_REPO_ROOT="$ROOT"
export SERENITY_MODEL_ROOT="${SERENITY_MODEL_ROOT:-$ROOT/models}"
export SERENITY_OUT_DIR="${SERENITY_OUT_DIR:-$ROOT/output/run}"
export DIFC_CONFIG="${DIFC_CONFIG:-$ROOT/config/difc.json}"
PORT=7811
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --port) PORT="$2"; shift 2;;
    *) ARGS+=("$1"); shift;;
  esac
done
mkdir -p "$SERENITY_OUT_DIR"
exec "$ROOT/serenity-server/target/release/serenity-server" \
  --worker "$ROOT/output/bin/serenity_worker_difc" --port "$PORT" --out-dir "$SERENITY_OUT_DIR" "${ARGS[@]}"
