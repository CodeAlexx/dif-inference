#!/usr/bin/env bash
# Explicit one-time native preparation; sealed receipts use Ref2VA's actual
# shard paths/content, never Base's structurally identical index JSON alone.
set -euo pipefail
root="${SERENITY_REPO_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
source "$root/scripts/config.sh"
export DIFC_CONFIG="$(config_get minimax_h3.task_configs.ref2va)"
source "$root/scripts/config.sh"
if [[ "${1:-}" != --inside-memory-cap ]]; then
  export MEM_MAX="$(config_get memory_max)" MEM_HIGH="$(config_get runtime.memory_high)"
  export SWAP_MAX="$(config_get memory_swap_max)" DESKTOP_RESERVE="$(config_get desktop_reserve)"
  exec "$(config_get runtime.memory_wrapper)" bash "$0" --inside-memory-cap "$@"
fi
shift
if [[ -z "${DIFC_LOCK_HELD:-}" ]]; then
  exec 9>"$(config_get gpu_lock)"
  flock -w "$(config_get gpu_lock_wait_seconds)" 9
fi
b="$(config_get compiler_build)"; checkpoint="$(config_get minimax_h3.checkpoint)"
fixture="$(config_get minimax_h3.fixture_dir)"
mkdir -p "$fixture"
stage="$(mktemp -d "$fixture/.prepare.XXXXXX")"
echo "[difc-h3] Ref2VA preparation evidence: $stage"
publish() { [[ -s "$1" ]] || return 70; ln "$1" "$2"; }
if [[ ! -s "$(config_get minimax_h3.conditioner_bundle)" ]]; then
  "$b/difcondition" program --checkpoint "$checkpoint" --sequence 1 --output "$stage/conditioner.difir"
  receipt_args=()
  if [[ -s "$(config_get minimax_h3.references.sealed_conditioner_source)" ]]; then
    receipt_args=(--sealed-bundle "$(config_get minimax_h3.references.sealed_conditioner_source)")
  fi
  "$b/difcondition" bundle --checkpoint "$checkpoint" --program "$stage/conditioner.difir" --output "$stage/conditioner.difbind" "${receipt_args[@]}"
  publish "$stage/conditioner.difbind" "$(config_get minimax_h3.conditioner_bundle)"
fi
if [[ ! -s "$(config_get minimax_h3.keyframes.vision_bundle)" ]]; then
  gh=$(( $(config_get minimax_h3.profile.height) / $(config_get minimax_h3.keyframes.vision_patch_size) ))
  gw=$(( $(config_get minimax_h3.profile.width) / $(config_get minimax_h3.keyframes.vision_patch_size) ))
  "$b/difh3vision" program --grid-t 1 --grid-h "$gh" --grid-w "$gw" --output "$stage/vision.difir"
  receipt_args=()
  if [[ -s "$(config_get minimax_h3.references.sealed_vision_source)" ]]; then
    receipt_args=(--sealed-bundle "$(config_get minimax_h3.references.sealed_vision_source)")
  fi
  "$b/difh3vision" bundle --checkpoint "$checkpoint/text_encoder" --program "$stage/vision.difir" \
    --grid-t 1 --grid-h "$gh" --grid-w "$gw" --output "$stage/vision.difbind" "${receipt_args[@]}"
  publish "$stage/vision.difbind" "$(config_get minimax_h3.keyframes.vision_bundle)"
fi
if [[ ! -s "$(config_get minimax_h3.denoiser_bundle)" ]]; then
  h=$(( $(config_get minimax_h3.profile.height) / 16 )); w=$(( $(config_get minimax_h3.profile.width) / 16 ))
  frames="$(config_get minimax_h3.profile.frames)"
  t=$(( (frames - 5) / 17 * 5 + 2 )); rows=$(( t * (h / 2) * (w / 2) ))
  audio=$(awk "BEGIN {printf \"%.0f\", $frames / $(config_get minimax_h3.profile.fps) * 40}")
  "$b/difc" make-h3-denoiser "$stage/denoiser.difir" "$rows" "$((audio * 2))" 1 \
    "$(config_get minimax_h3.profile.timestep_tables)" streamed "$(config_get minimax_h3.profile.program_attention)"
  "$b/difweights" make-h3-denoiser-bundle "$(config_get minimax_h3.transformer_index)" "$stage/denoiser.difir" "$stage/denoiser.difbind"
  publish "$stage/denoiser.difbind" "$(config_get minimax_h3.denoiser_bundle)"
fi
if [[ ! -s "$(config_get minimax_h3.modulation_cache)" ]]; then
  "$b/difmodcache" --checkpoint-index "$(config_get minimax_h3.transformer_index)" \
    --schedule-points "$(config_get minimax_h3.profile.steps)" --steps "$(config_get minimax_h3.profile.steps)" \
    --condition-video-floor "$(config_get minimax_h3.references.condition_timestep)" \
    --engine cuda --cublas "$(config_get minimax_h3.preparation.cublas)" --output "$stage/modcache.safetensors"
  publish "$stage/modcache.safetensors" "$(config_get minimax_h3.modulation_cache)"
fi
if jq -e '.minimax_h3.references.kinds | index("audio") != null' <<<"$DIFC_DOCUMENT" >/dev/null &&
   [[ ! -s "$(config_get minimax_h3.references.audio_encoder.modulation_cache)" ]]; then
  "$b/difmodcache" --checkpoint-index "$(config_get minimax_h3.transformer_index)" \
    --schedule-points "$(config_get minimax_h3.profile.steps)" --steps "$(config_get minimax_h3.profile.steps)" \
    --condition-video-floor "$(config_get minimax_h3.references.condition_timestep)" \
    --condition-audio-timestep "$(config_get minimax_h3.references.audio_encoder.condition_timestep)" \
    --engine cuda --cublas "$(config_get minimax_h3.preparation.cublas)" --output "$stage/audio-modcache.safetensors"
  publish "$stage/audio-modcache.safetensors" "$(config_get minimax_h3.references.audio_encoder.modulation_cache)"
fi
if [[ "$(config_get minimax_h3.int8_route)" == convrot && ! -s "$(config_get minimax_h3.convrot_int8)" ]]; then
  "$b/difh3convrot" "$(config_get minimax_h3.transformer_index)" "$stage/convrot.safetensors" \
    --layers "$(config_get minimax_h3.profile.blocks)"
  publish "$stage/convrot.safetensors" "$(config_get minimax_h3.convrot_int8)"
fi
"$b/difweights" inspect-bundle "$(config_get minimax_h3.denoiser_bundle)" > "$stage/denoiser-receipts.txt"
"$b/difweights" inspect-bundle "$(config_get minimax_h3.conditioner_bundle)" > "$stage/conditioner-receipts.txt"
"$DIFC_CONFIG_TOOL" check-h3 --task ref2va
