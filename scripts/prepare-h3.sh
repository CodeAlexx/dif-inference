#!/usr/bin/env bash
# Prepare only the configured H3 model/schedule cache. No prompt or weights
# are downloaded, and existing files are never replaced.
set -euo pipefail
script_root="$(cd "$(dirname "$0")/.." && pwd)"
source "$script_root/scripts/config.sh"
compiler_bin="$(config_get compiler_build)"
cache_path="$(config_get minimax_h3.modulation_cache)"
if [[ -s "$cache_path" ]]; then
  echo "[difc-h3] prepared cache exists: $cache_path"
  "$DIFC_CONFIG_TOOL" check-h3
  exit 0
fi
[[ "$(config_get minimax_h3.profile.task)" == t2va ]] || {
  echo 'H3 preparation requires an explicitly configured task cache recipe.' >&2
  exit 65
}
[[ -x "$compiler_bin/difmodcache" ]] || { echo "Missing native cache builder: $compiler_bin/difmodcache" >&2; exit 66; }
[[ -f "$(config_get minimax_h3.preparation.cublas)" ]] || { echo 'Configured cache-builder cuBLAS library is missing.' >&2; exit 66; }
mkdir -p "$(dirname "$cache_path")"
# Native builder and publication share one GPU lease. Recheck after taking it
# so two launchers cannot build or overwrite the same cache concurrently.
if [[ -z "${DIFC_LOCK_HELD:-}" ]]; then
  exec 9>"$(config_get gpu_lock)"
  flock -w "$(config_get gpu_lock_wait_seconds)" 9
fi
if [[ ! -e "$cache_path" ]]; then
  cache_stage="$(mktemp -d "$(dirname "$cache_path")/.prepare-h3.XXXXXX")"
  export MEM_MAX="$(config_get memory_max)"
  export MEM_HIGH="$(config_get runtime.memory_high)"
  export SWAP_MAX="$(config_get memory_swap_max)"
  export DESKTOP_RESERVE="$(config_get desktop_reserve)"
  echo "[difc-h3] building native T2VA modulation cache; staging retained at $cache_stage"
  "$(config_get runtime.memory_wrapper)" "$compiler_bin/difmodcache" \
    --checkpoint-index "$(config_get minimax_h3.transformer_index)" \
    --schedule-points "$(config_get minimax_h3.profile.steps)" \
    --steps "$(config_get minimax_h3.profile.steps)" \
    --engine cuda --cublas "$(config_get minimax_h3.preparation.cublas)" \
    --output "$cache_stage/cache.safetensors"
  [[ -s "$cache_stage/cache.safetensors" ]] || { echo 'Native builder produced no cache.' >&2; exit 70; }
  # Hard-link publication is atomic and refuses an already-existing target.
  ln "$cache_stage/cache.safetensors" "$cache_path"
fi
"$DIFC_CONFIG_TOOL" check-h3
