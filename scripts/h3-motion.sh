#!/usr/bin/env bash
# Mojo's motion extension uses target noise drawn by the ordinary T2VA route
# plus a fixed native tail. Never encode the preceding decoded MP4 again.
h3_motion_conditioning() {
  local context_steps context_audio
  context_steps=$(((MOTION_FRAMES - 5) / 17 * 5 + 2))
  context_audio=$(python3 -c "print(round($MOTION_FRAMES / $FPS * 40))")
  "${GPU[@]}" "$B/difh3noise" --rows "$((context_steps * ROWS_PER_FRAME))" --cols 96 \
    --seed "$((SEED + $(jq_get minimax_h3.motion.noise_seed_offset)))" --output motion-condition-noise.diftensor
  MOTION_ARGS=(--motion-context "$MOTION_CONTEXT" --motion-context-frames "$MOTION_FRAMES" \
    --motion-condition-noise motion-condition-noise.diftensor)
  # The driver's public video/audio inputs remain target-only noise. These
  # counts are for its expanded graph; the driver prepends fixed context once.
  VIDEO_ROWS=$((VIDEO_ROWS + context_steps * ROWS_PER_FRAME))
  AUDIO_ROWS=$((AUDIO_ROWS + context_audio * 2))
  TIMESTEP_TABLES="$(jq_get minimax_h3.motion.timestep_tables)"
  MODCACHE="$(jq_get minimax_h3.motion.modulation_cache)"
}

h3_motion_audio_program() {
  # Length-dependent block-average constants change with the timeline. Rebuild
  # those constants only; reuse sealed learned-weight receipts without rescanning
  # the full checkpoint on every request. Generic rebind-program cannot do this.
  "$B/difimport" make-audio-program motion-audio.difir \
    "$(jq_get minimax_h3.motion.audio_decoder_batch)" "$AUDIO_LATENTS" \
    "$(jq_get minimax_h3.motion.audio_decoder_stages)"
  "$B/difimport" rebind-audio-bundle "$(jq_get minimax_h3.audio_bundle)" \
    "$(jq_get minimax_h3.audio_program)" motion-audio.difir \
    motion-audio-generated.safetensors motion-audio.difbind \
    "$(jq_get minimax_h3.motion.audio_decoder_batch)" "$AUDIO_LATENTS" \
    "$(jq_get minimax_h3.motion.audio_decoder_stages)"
  AUDIO_PROGRAM=motion-audio.difir
  AUDIO_BUNDLE=motion-audio.difbind
}
