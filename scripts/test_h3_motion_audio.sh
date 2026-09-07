#!/usr/bin/env bash
# Real native audio-geometry preparation using deployment receipts. No neural
# execution and no checkpoint rehash. Run under mem_safe_runtime.sh.
set -euo pipefail
root="${SERENITY_REPO_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
source "$root/scripts/config.sh"
source "$(config_get minimax_h3.motion.stage_script)"
jq_get() { config_get "$1"; }
B="$(config_get compiler_build)"
lab="$(mktemp -d "$root/output/h3-motion-audio-gate.XXXXXX")"
windows="$(jq -er '.minimax_h3.motion.windows | select(type == "array" and length > 0) | .[]' <<<"$DIFC_DOCUMENT")"
for overlap in $windows; do
  frames=$(( $(config_get minimax_h3.profile.frames) + overlap ))
  frames=$(( ((frames - 5 + 16) / 17) * 17 + 5 ))
  AUDIO_LATENTS=$(python3 -c "print(round($frames / $(config_get minimax_h3.profile.fps) * 40))")
  mkdir "$lab/overlap-$overlap"
  (
    cd "$lab/overlap-$overlap"
    h3_motion_audio_program
    [[ -s "$AUDIO_PROGRAM" && -s "$AUDIO_BUNDLE" && -s motion-audio-generated.safetensors ]]
    echo "MOTION_AUDIO_PREP PASS overlap=$overlap target_frames=$frames audio_latents=$AUDIO_LATENTS"
  )
done
echo "MOTION_AUDIO_PREP evidence=$lab (preparation only; no decoded audio gate)"
