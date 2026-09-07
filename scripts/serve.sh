#!/usr/bin/env bash
# Launch the diffusion-compiler-ui server (forked serenity-server) with the
# compiler-backed worker. Port 7811 by default so it never collides with the
# mojodiffusion server on 7801.
#   scripts/serve.sh [--port N] [extra serenity-server args]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/config.sh"
export SERENITY_MODEL_ROOT="$(config_get server.model_root)"
export SERENITY_OUT_DIR="$(config_get server.output_dir)"
PORT="$(config_get server.port)"
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --port) PORT="$2"; shift 2;;
    *) ARGS+=("$1"); shift;;
  esac
done
mkdir -p "$SERENITY_OUT_DIR"
exec "$(config_get server.binary)" \
  --worker "$(config_get server.worker)" --port "$PORT" --out-dir "$SERENITY_OUT_DIR" "${ARGS[@]}"
