#!/usr/bin/env bash
# Build this checkout's Rust control plane and compiler-backed worker.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
# Bootstrap only the tiny native JSON reader. The bootstrap memory envelope
# is explicit in runtime.json; the full build uses the selected machine JSON.
export MEM_MAX="$(jq -er '.memory_max' config/runtime.json)"
export MEM_HIGH="$(jq -er '.runtime.memory_high' config/runtime.json)"
export SWAP_MAX="$(jq -er '.memory_swap_max' config/runtime.json)"
export DESKTOP_RESERVE="$(jq -er '.desktop_reserve' config/runtime.json)"
scripts/mem_safe_runtime.sh cargo build --release -p difc-config \
  --manifest-path serenity-server/Cargo.toml -j "$(jq -er '.build.jobs' config/runtime.json)"
source "$repo_root/scripts/config.sh"
export MEM_MAX="$(config_get memory_max)" MEM_HIGH="$(config_get runtime.memory_high)"
export SWAP_MAX="$(config_get memory_swap_max)" DESKTOP_RESERVE="$(config_get desktop_reserve)"
"$(config_get build.memory_wrapper)" cargo build --release --workspace \
  --manifest-path "$(config_get build.manifest)" --target-dir "$(config_get build.target_dir)" -j "$(config_get build.jobs)"
install -Dm755 "$(config_get build.target_dir)/release/serenity_worker_difc" "$(config_get server.worker)"
# The same native worker implements the CPU-only IPC stub, no Mojo binary.
if [[ ! -e output/bin/serenity_worker_stub && ! -L output/bin/serenity_worker_stub ]]; then
  ln -s serenity_worker_difc output/bin/serenity_worker_stub
fi
