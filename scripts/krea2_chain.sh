#!/usr/bin/env bash
# Krea 2 (Turbo / Raw) prompt -> 1024x1024 PNG through the Diffusion Compiler's
# native chain, from RAW checkpoints (no sealed bundles) with a native seeded
# initial latent (difkrea2sample --seed, torch-CPU-generator convention). The
# tool's parser accepts --seed only; --initial-seed was rejected outright with
# "invalid difkrea2sample argument", so every Krea render died after the
# conditioner had already run.
#   krea2_chain.sh OUT_DIR PROMPT_FILE SEED STEPS GUIDANCE [turbo|raw] [NEGATIVE_FILE] [--stop-after N]
set -euo pipefail
ROOT="${SERENITY_REPO_ROOT:-$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)}"
source "$ROOT/scripts/config.sh"
jq_get() { config_get "$1"; }
B="$(jq_get compiler_build)"
OUT="${1:?out_dir}"; PROMPT_FILE="${2:?prompt file}"; SEED="${3:?seed}"; STEPS="${4:?steps}"; GUIDANCE="${5:?guidance}"
VARIANT="${6:-turbo}"; NEG_FILE="${7:-}"; STOP_AFTER="${8:-}"
# Authored geometry. difkrea2sample derives the latent grid from these, so the
# chain is not restricted to any single compiled shape.
WIDTH="${9:-1024}"; HEIGHT="${10:-1024}"
QWEN="$(jq_get krea2.qwen_processor)"; COND_DIFIR="$(jq_get krea2.conditioner_program)"; COND_BUNDLE="$(jq_get krea2.conditioner_bundle)"
VAE_CKPT="$(jq_get krea2.vae_checkpoint)"; VAE_CONFIG="$(jq_get krea2.vae_config)"; CACHE="$(jq_get krea2.cache_dir)"
RESIDENT_MIB="$(jq_get krea2.resident_plan_mib)"; MU="$(jq_get krea2.mu)"
case "$VARIANT" in turbo) CKPT="$(jq_get krea2.turbo_checkpoint)";; raw) CKPT="$(jq_get krea2.raw_checkpoint)";; *) echo "FATAL variant $VARIANT" >&2; exit 65;; esac
# The difc worker already holds the GPU lock around the whole stage; taking it
# again here would deadlock. Lock only when invoked standalone.
if [[ -n "${DIFC_LOCK_HELD:-}" ]]; then GPU=(); else GPU=(flock -w "$(jq_get gpu_lock_wait_seconds)" "$(jq_get gpu_lock)"); fi
mkdir -p "$OUT"; cd "$OUT"
log() { echo "[difc-krea2] $*"; }
log "phase=tokenize"
"$B/diftokenize" --processor "$QWEN" --prompt-file "$PROMPT_FILE" --strip-trailing-newline --krea2-inputs-out tokenizer.safetensors
NEG=()
if [[ -n "$NEG_FILE" ]]; then
  "$B/diftokenize" --processor "$QWEN" --prompt-file "$NEG_FILE" --strip-trailing-newline --krea2-inputs-out neg-tokenizer.safetensors
fi
log "phase=conditioning (Qwen3-VL-4B, 546 tokens)"
"${GPU[@]}" "$B/difcondition" run --krea2 --program "$COND_DIFIR" --bundle "$COND_BUNDLE" --inputs tokenizer.safetensors --output taps.safetensors --report qwen-report.json --resident-streamed --cache-dir "$CACHE" --min-free-mib "$(jq_get krea2.min_free_mib)"
if [[ -n "$NEG_FILE" ]]; then
  "${GPU[@]}" "$B/difcondition" run --krea2 --program "$COND_DIFIR" --bundle "$COND_BUNDLE" --inputs neg-tokenizer.safetensors --output neg-taps.safetensors --report neg-qwen-report.json --resident-streamed --cache-dir "$CACHE" --min-free-mib "$(jq_get krea2.min_free_mib)"
fi
log "phase=text-fusion"
"${GPU[@]}" "$B/difkrea2text" --checkpoint "$CKPT" --taps taps.safetensors --mask-inputs tokenizer.safetensors --no-compare --output conditioning.safetensors --report text-report.json --diffir text.difir
if [[ -n "$NEG_FILE" ]]; then
  "${GPU[@]}" "$B/difkrea2text" --checkpoint "$CKPT" --taps neg-taps.safetensors --mask-inputs neg-tokenizer.safetensors --no-compare --output neg-conditioning.safetensors --report neg-text-report.json --diffir neg-text.difir
  NEG=(--negative-conditioning neg-conditioning.safetensors --negative-tokenizer neg-tokenizer.safetensors)
fi
STOP=(); [[ -n "$STOP_AFTER" ]] && STOP=(--stop-after "$STOP_AFTER")
log "phase=denoise steps=$STEPS guidance=$GUIDANCE seed=$SEED variant=$VARIANT resident_mib=$RESIDENT_MIB"
"${GPU[@]}" "$B/difkrea2sample" --checkpoint "$CKPT" --positive-conditioning conditioning.safetensors --positive-tokenizer tokenizer.safetensors "${NEG[@]}" \
  --initial-seed "$SEED" --width "$WIDTH" --height "$HEIGHT" \
  --output sampler.safetensors --report sampler-report.json --diffir sampler.difir \
  --steps "$STEPS" --guidance "$GUIDANCE" --mu "$MU" --seed "$SEED" "${STOP[@]}" \
  --resident-plan-mib "$RESIDENT_MIB" --resident-order "$(jq_get krea2.resident_order)" --alias-reshapes --lazy-resident-upload --streamed-stage-threads "$(jq_get krea2.streamed_stage_threads)" --cache-dir "$CACHE"
if [[ -n "$STOP_AFTER" ]]; then log "stopped after $STOP_AFTER (diagnostic)"; exit 0; fi
log "phase=vae decode"
"${GPU[@]}" "$B/difkrea2vae" --checkpoint "$VAE_CKPT" --sampler sampler.safetensors --config "$VAE_CONFIG" --png image.png --output vae.safetensors --report vae-report.json --diffir vae.difir
log "done $OUT/image.png"
