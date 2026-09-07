#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# VENDORED into diffusion-compiler scripts/ on 2026-08-31 (w1-deps step 1).
# Canonical source: /home/alex/mojodiffusion/scripts/mem_safe_runtime.sh
#   (sha256 50a8bc29aac53240bba0cb7e7b7b1ed66e54f82687035b0da89a1d2cfa3c81d5,
#    mtime 2026-08-18) — the copy referenced by the accepted H3 quality-gate
#   artifact scripts (artifacts/h3-quality-natural-language-2026-08-30/…/run_*.sh).
# A second, older copy exists at /home/alex/mojo1-migration/scripts/
#   mem_safe_runtime.sh (sha256 1e7a5790…, 2026-08-12 snapshot); it predates the
#   H3-training admission gate (H3_ALLOW_USER_SLICE) and the clean-service env
#   forwarding (CONDA/MODULAR/LD_LIBRARY_PATH + MODULAR_DEVICE_CONTEXT_*), so
#   the mojodiffusion copy is canonical and is what is vendored here.
# Local changes add configuration forwarding and aggregate pressure monitoring.
# This file is no longer byte-identical to the original vendored source.
# ─────────────────────────────────────────────────────────────────────────────
# Run a large GPU runtime in a rootless, hard-capped transient user service.
#
# This is deliberately different from mem_safe.sh.  Large user scopes with a
# low MemoryHigh caused sustained reclaim pressure under user@1000.service and
# allowed systemd-oomd to kill the desktop.  This wrapper leaves MemoryHigh at
# infinity, applies a finite MemoryMax to the complete process tree, enables
# cgroup OOM grouping, and refuses admission unless the host can retain a large
# desktop reserve even at the child's hard ceiling.
#
# Usage: scripts/mem_safe_runtime.sh <program> [args...]
set -euo pipefail

guard_root="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
guard_policy="$guard_root/config/runtime.json"
guard_script="$guard_root/scripts/runtime_guard.py"
guard_lock="$(jq -er '.runtime.memory_guard.lock_file | select(type == "string" and length > 0)' "$guard_policy")"
# This is a HEAVY-WORK lock, distinct from the device lock: CPU hashing,
# builds and oracle runs can create the same session pressure as CUDA work.
# Refuse overlap immediately; do not queue an unbudgeted job behind another.
exec {guard_lock_fd}>"$guard_lock"
flock -n "$guard_lock_fd" || { echo 'mem_safe_runtime: another guarded heavy job is active' >&2; exit 75; }

MEM_MAX="${MEM_MAX:-24G}"
MEM_HIGH="${MEM_HIGH:-infinity}"
SWAP_MAX="${SWAP_MAX:-2G}"
DESKTOP_RESERVE="${DESKTOP_RESERVE:-16G}"
# Static ceiling on the per-job cgroup cap. This is a backstop only: the two
# gates below already refuse dynamically against live MemAvailable and the
# desktop reserve, and the guard policy admits separately, so a job can never
# actually take memory the host does not have at that moment.
#
# 24G was too low for image work and was silently costing wall time rather than
# protecting anything. cgroup-v2 charges page cache to the reading cgroup, so a
# FLUX.2 klein 9B job holding ~20 GB of device-staging plus an 18.16 GB
# checkpoint read exceeds 24G and the kernel reclaims the very pages it is
# reading. Measured on this box, identical binary and flags, warm cache:
#   unwrapped            75.0 s total, 20.6 s resident upload
#   wrapped MemoryMax=24G 163.9 s total, 109.5 s resident upload
# 62 GB host minus the 16G desktop reserve leaves 46G, so 44G keeps the reserve
# intact while letting an 18 GB checkpoint and its staging coexist.
RUNTIME_MAX="${RUNTIME_MAX:-44G}"

if [[ $# -lt 1 ]]; then
  echo "mem_safe_runtime: usage: $0 <program> [args...]" >&2
  exit 64
fi

max_bytes="$(numfmt --from=iec "$MEM_MAX")" || {
  echo "mem_safe_runtime: invalid MEM_MAX: $MEM_MAX" >&2
  exit 64
}
runtime_max_bytes="$(numfmt --from=iec "$RUNTIME_MAX")"
reserve_bytes="$(numfmt --from=iec "$DESKTOP_RESERVE")" || {
  echo "mem_safe_runtime: invalid DESKTOP_RESERVE: $DESKTOP_RESERVE" >&2
  exit 64
}
swap_bytes="$(numfmt --from=iec "$SWAP_MAX")" || {
  echo "mem_safe_runtime: invalid SWAP_MAX: $SWAP_MAX" >&2
  exit 64
}
if (( max_bytes > runtime_max_bytes )); then
  echo "mem_safe_runtime: refusing MEM_MAX=$MEM_MAX (maximum $RUNTIME_MAX)" >&2
  exit 78
fi
if (( swap_bytes > 2 * 1024 * 1024 * 1024 )); then
  echo "mem_safe_runtime: refusing SWAP_MAX=$SWAP_MAX (maximum 2G)" >&2
  exit 78
fi
if [[ "$MEM_HIGH" != "infinity" && "$MEM_HIGH" != "max" ]]; then
  high_bytes="$(numfmt --from=iec "$MEM_HIGH")" || {
    echo "mem_safe_runtime: invalid MEM_HIGH: $MEM_HIGH" >&2
    exit 64
  }
  if (( high_bytes < max_bytes )); then
    echo "mem_safe_runtime: MemoryHigh below MemoryMax is unsafe in the user slice" >&2
    echo "mem_safe_runtime: use MEM_HIGH=infinity (the default)" >&2
    exit 78
  fi
fi

mem_total_kib="$(awk '/^MemTotal:/ {print $2}' /proc/meminfo)"
mem_available_kib="$(awk '/^MemAvailable:/ {print $2}' /proc/meminfo)"
mem_total_bytes=$((mem_total_kib * 1024))
mem_available_bytes=$((mem_available_kib * 1024))
uid="$(id -u)"
user_service="/sys/fs/cgroup/user.slice/user-${uid}.slice/user@${uid}.service"
if [[ ! -r "$user_service/memory.stat" ]]; then
  echo "mem_safe_runtime: cannot inspect user-service memory accounting" >&2
  exit 77
fi
user_anon_bytes="$(awk '$1 == "anon" {print $2}' "$user_service/memory.stat")"
user_shmem_bytes="$(awk '$1 == "shmem" {print $2}' "$user_service/memory.stat")"
user_kernel_bytes="$(awk '$1 == "kernel" {print $2}' "$user_service/memory.stat")"
user_nonreclaimable_bytes=$((user_anon_bytes + user_shmem_bytes + user_kernel_bytes))

# Both gates are intentional.  MemAvailable protects against unrelated system
# use; user anon/shmem/kernel + child-max protects the desktop even if every
# admitted byte is touched before the kernel's child-cgroup OOM kill fires.
# Clean checkpoint page cache is deliberately excluded from the second gate: it
# remains included in MemAvailable and is reclaimable, while counting it as live
# desktop memory would permanently lock out the next benchmark after one mmap.
if (( mem_available_bytes < max_bytes + reserve_bytes )); then
  echo "mem_safe_runtime: insufficient host headroom for $MEM_MAX + $DESKTOP_RESERVE reserve" >&2
  exit 75
fi
if (( user_nonreclaimable_bytes + max_bytes > mem_total_bytes - reserve_bytes )); then
  echo "mem_safe_runtime: user session + $MEM_MAX would violate $DESKTOP_RESERVE reserve" >&2
  exit 75
fi
python3 "$guard_script" admit --policy "$guard_policy" --parent "$user_service" \
  --max-bytes "$max_bytes" --reserve-bytes "$reserve_bytes"

prog="$1"
shift
prog_path="$(command -v "$prog")" || {
  echo "mem_safe_runtime: '$prog' not on PATH" >&2
  exit 127
}
if [[ "$prog_path" != /* ]]; then
  prog_path="$(realpath "$prog_path")"
fi

# A long-lived H3 training process still needs an explicit rootless opt-in. The
# 2026-08-16 guided run proved that a 24G child cap with the child left at
# ManagedOOMMemoryPressure=auto was not enough: oomd selected the parent user
# service at step 389. Every guarded child now has its own pressure-kill policy
# in addition to the host/ancestor watcher. Training remains a separate opt-in.
managed_oom_pressure=kill
if [[ "$(basename "$prog_path")" == "train_minimax_h3" ]]; then
  if [[ "${H3_ALLOW_USER_SLICE:-0}" != 1 ]]; then
    echo "mem_safe_runtime: H3 training requires H3_ALLOW_USER_SLICE=1" >&2
    echo "mem_safe_runtime: use the bounded H3 trainer launcher" >&2
    exit 78
  fi
  managed_oom_pressure=kill
  echo "mem_safe_runtime: rootless H3 training explicitly admitted" >&2
fi

unit_name="serenity-runtime-memory-$(date +%Y%m%d-%H%M%S)-$$"
unit_cgroup="${user_service}/app.slice/${unit_name}.service"

runtime_runner_pid=""
runtime_guard_pid=""
cleanup_runtime() {
  if [[ -n "$runtime_runner_pid" ]]; then
    systemctl --user kill --kill-whom=all --signal=SIGKILL "$unit_name" 2>/dev/null || true
    kill "$runtime_runner_pid" 2>/dev/null || true
    wait "$runtime_runner_pid" 2>/dev/null || true
  fi
  if [[ -n "$runtime_guard_pid" ]]; then
    kill "$runtime_guard_pid" 2>/dev/null || true
    wait "$runtime_guard_pid" 2>/dev/null || true
  fi
}
trap cleanup_runtime EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# A --user *service* starts with a clean environment (unlike mem_safe.sh's
# env-inheriting scope). Forward the toolchain roots too, so `mojo build`
# under this wrapper can resolve std/max — builds moved here after the
# 2026-08-13 oomd session kill (low-MemoryHigh scope reclaim; MJ-1140).
extra_env=()
[[ -n "${CONDA_PREFIX:-}" ]] && extra_env+=(--setenv="CONDA_PREFIX=$CONDA_PREFIX")
[[ -n "${MODULAR_HOME:-}" ]] && extra_env+=(--setenv="MODULAR_HOME=$MODULAR_HOME")
[[ -n "${LD_LIBRARY_PATH:-}" ]] && extra_env+=(--setenv="LD_LIBRARY_PATH=$LD_LIBRARY_PATH")
# DeviceContext reads these before its singleton allocator is constructed.
# Forward explicit caller policy across the clean `systemd-run --user` service
# boundary; without this allow-list the values are silently lost and MAX falls
# back to its large default arena, leaving too little VRAM for desktop clients.
for env_name in \
  DIFC_CONFIG \
  SERENITY_REPO_ROOT \
  SERENITY_MODEL_ROOT \
  H3_DENSE_INT8_KERNEL_PATH \
  DIFC_LOCK_HELD \
  DIF_FASTLOAD \
  CUDA_CACHE_PATH \
  CUDA_MODULE_LOADING \
  LD_BIND_NOW \
  CUDA_FORCE_PRELOAD_LIBRARIES \
  MODULAR_DEVICE_CONTEXT_MEMORY_MANAGER_SIZE \
  MODULAR_DEVICE_CONTEXT_MEMORY_MANAGER_SIZE_PERCENT \
  MODULAR_DEVICE_CONTEXT_MEMORY_MANAGER_CHUNK_PERCENT \
  MODULAR_DEVICE_CONTEXT_HOST_MEMORY_MANAGER_SIZE \
  MODULAR_DEVICE_CONTEXT_HOST_MEMORY_MANAGER_CHUNK_PERCENT \
  MODULAR_DEVICE_CONTEXT_MEMORY_MANAGER_LOG
do
  if [[ -n "${!env_name:-}" ]]; then
    extra_env+=(--setenv="$env_name=${!env_name}")
  fi
done

systemd-run --user \
  --quiet --wait --collect --pipe --service-type=exec \
  --unit="$unit_name" \
  --working-directory="$PWD" \
  --property="MemoryHigh=$MEM_HIGH" \
  --property="MemoryMax=$MEM_MAX" \
  --property="MemorySwapMax=$SWAP_MAX" \
  --property=OOMPolicy=kill \
  --property="ManagedOOMMemoryPressure=$managed_oom_pressure" \
  --property="ManagedOOMMemoryPressureLimit=$(jq -er '.runtime.memory_guard.some_pressure_percent' "$guard_policy")%" \
  --property="RuntimeMaxSec=$(jq -er '.runtime.memory_guard.maximum_runtime_seconds' "$guard_policy")" \
  --setenv="PATH=$PATH" \
  "${extra_env[@]}" \
  -- "$prog_path" "$@" &
runtime_runner_pid="$!"

set +e
python3 "$guard_script" watch --policy "$guard_policy" --parent "$user_service" \
  --max-bytes "$max_bytes" --reserve-bytes "$reserve_bytes" \
  --unit "$unit_name" --child "$unit_cgroup" --runner-pid "$runtime_runner_pid" --owner-pid "$$" &
runtime_guard_pid="$!"
# Bash wait is interruptible by traps; a foreground Python command would defer
# TERM handling until that command finished. The watcher independently detects
# wrapper death (including untrappable SIGKILL).
wait "$runtime_guard_pid"
guard_rc="$?"
runtime_guard_pid=""
if [[ "$guard_rc" != 0 ]]; then
  systemctl --user kill --kill-whom=all --signal=SIGKILL "$unit_name" 2>/dev/null || true
fi
wait "$runtime_runner_pid"
runner_rc="$?"
set -e
runtime_runner_pid=""
trap - INT TERM EXIT
echo "[mem_safe_runtime] unit=$unit_name guard_rc=$guard_rc child_rc=$runner_rc" >&2
if [[ "$guard_rc" != 0 ]]; then exit "$guard_rc"; fi
exit "$runner_rc"
