#!/usr/bin/env bash
# H3 frontend orchestration, sourced by h3_compiler_runner.sh. All neural work
# uses the existing native compiler tools and their shared runtime. Inputs are
# already-normalized PNGs from the control plane; no synthetic conditioning.
h3_keyframe_conditioning() {
  local target_video_rows="$VIDEO_ROWS"
  local grid_h=$((H / $(jq_get minimax_h3.keyframes.vision_patch_size)))
  local grid_w=$((W / $(jq_get minimax_h3.keyframes.vision_patch_size)))
  local merge_size="$(jq_get minimax_h3.keyframes.vision_merge_size)"
  local vision_tokens=$((grid_h * grid_w * ${#KEYFRAME_IMAGES[@]} / (merge_size * merge_size)))
  local -a image_args=() state_args=()
  local -a previous_draws=()
  local path index=0
  for path in "${KEYFRAME_IMAGES[@]}"; do image_args+=(--image "$path"); done
  MODCACHE="$(jq_get minimax_h3.keyframes.modulation_cache)"
  TIMESTEP_TABLES="$(jq_get minimax_h3.keyframes.timestep_tables)"
  log "phase=keyframe-presentation task=$TASK images=${#KEYFRAME_IMAGES[@]}"
  "$B/difh3vision" inputs --checkpoint "$CKPT/text_encoder" \
    --processor "$(jq_get minimax_h3.processor)" "${image_args[@]}" \
    --prompt-file prompt.txt --grid-t 1 --grid-h "$grid_h" --grid-w "$grid_w" \
    --strip-trailing-newline --output presentation.safetensors \
    --ids-out ids.diftensor --tags-out token-tags.diftensor
  read -r T < <(od -An -tu8 -j20 -N8 ids.diftensor)
  [[ "$T" =~ ^[1-9][0-9]*$ ]] || { echo 'FATAL invalid keyframe token count' >&2; return 65; }
  log "phase=vision text_tokens=$T visual_tokens=$vision_tokens"
  "${GPU[@]}" "$B/difh3vision" run \
    --program "$(jq_get minimax_h3.keyframes.vision_program)" \
    --bundle "$(jq_get minimax_h3.keyframes.vision_bundle)" \
    --inputs presentation.safetensors --grid-t 1 --grid-h "$grid_h" --grid-w "$grid_w" \
    --output vision.safetensors --backend cuda --cache-dir "$CACHE" \
    --min-free-mib "$MIN_FREE" --streamed-stage-threads "$(jq_get minimax_h3.policy.streamed_stage_threads)" \
    --report vision.json
  log 'phase=prepare multimodal conditioner program + rebind'
  "$B/difcondition" program --checkpoint "$CKPT" --sequence "$T" \
    --vision-tokens "$vision_tokens" --output "cond-s$T.difir" > prep.log
  "$B/difcondition" bundle --checkpoint "$CKPT" --program "cond-s$T.difir" \
    --vision-tokens "$vision_tokens" --output "cond-s$T.difbind" \
    --sealed-bundle "$(jq_get minimax_h3.conditioner_bundle)" >> prep.log
  log 'phase=conditioning multimodal'
  "${GPU[@]}" "$B/difcondition" run --program "cond-s$T.difir" --bundle "cond-s$T.difbind" \
    --vision-inputs presentation.safetensors --vision-outputs vision.safetensors \
    --vision-tokens "$vision_tokens" --output conditioning.diftensor \
    --streamed-stage-threads "$(jq_get minimax_h3.policy.streamed_stage_threads)" \
    --min-free-mib "$MIN_FREE" --cache-dir "$CACHE" --report conditioner.json
  log "REAL conditioning: done tokens=$T visual_tokens=$vision_tokens"
  for path in "${KEYFRAME_IMAGES[@]}"; do
    log "phase=keyframe-encode image=$index"
    "${GPU[@]}" "$B/difh3encode" --backend cuda \
      --program "$(jq_get minimax_h3.keyframes.encoder_program)" \
      --weight-bundle "$(jq_get minimax_h3.keyframes.encoder_bundle)" --image "$path" \
      --pixels-id "$(jq_get minimax_h3.keyframes.encoder_pixels_id)" \
      --moments-id "$(jq_get minimax_h3.keyframes.encoder_moments_id)" \
      --output-moments "keyframe-$index-moments.diftensor" \
      --output-latent "keyframe-$index-latent.diftensor" --output-rows "keyframe-$index-rows.diftensor" \
      --tile-size "$(jq_get minimax_h3.decoder.tile_size)" --tile-overlap "$(jq_get minimax_h3.decoder.tile_overlap)" \
      --posterior-seed "$(jq_get minimax_h3.keyframes.posterior_seed)" --cache-dir "$CACHE" --min-free-mib "$MIN_FREE"
    "$B/difh3noise" --rng torch-cpu --layout h3-video --latent-frames 1 \
      --latent-height "$LAT_H" --latent-width "$LAT_W" --seed "$SEED" "${previous_draws[@]}" \
      --output "keyframe-$index-noise.diftensor"
    previous_draws+=(--skip-normal-draw "$((ROWS_PER_FRAME * 96))")
    state_args+=(--condition "keyframe-$index-rows.diftensor" --condition-noise "keyframe-$index-noise.diftensor")
    index=$((index + 1))
  done
  "$B/difh3noise" --rng torch-cpu --layout h3-video --latent-frames "$LAT_T" \
    --latent-height "$LAT_H" --latent-width "$LAT_W" --seed "$SEED" "${previous_draws[@]}" --output target-video-noise.diftensor
  previous_draws+=(--skip-normal-draw "$((target_video_rows * 96))")
  "$B/difh3noise" --rng torch-cpu --layout flat --rows "$AUDIO_ROWS" --cols 32 \
    --seed "$SEED" "${previous_draws[@]}" --output audio-noise.diftensor
  "$B/difh3state" "${state_args[@]}" --target-noise target-video-noise.diftensor \
    --condition-timestep "$(jq_get minimax_h3.keyframes.condition_timestep)" --output video-noise.diftensor
  VIDEO_ROWS=$((target_video_rows + ${#KEYFRAME_IMAGES[@]} * ROWS_PER_FRAME))
  TEXT_ARGS=(--text-tags token-tags.diftensor)
  case "$TASK" in
    i2va) LAYOUT_ARGS=(--keyframes first);;
    l2va) LAYOUT_ARGS=(--keyframes last);;
    fl2va) LAYOUT_ARGS=(--keyframes first-last);;
    *) echo "FATAL invalid keyframe task=$TASK" >&2; return 65;;
  esac
}
