#!/usr/bin/env bash
# Ordered image/audio references. No neural work outside the shared
# compiler tools. The creator-sized VAE/vision canvas is distinct from output.

h3_reference_audio_preflight() {
  local path="$1" probe
  probe="$("$(jq_get minimax_h3.references.audio_encoder.ffprobe)" -v error \
    -select_streams a:0 -show_entries stream=duration:format=duration -of json "$path")" || return 65
  jq -e --argjson low "$(jq_get minimax_h3.references.audio_encoder.min_duration_seconds)" \
    --argjson high "$(jq_get minimax_h3.references.audio_encoder.max_duration_seconds)" \
    '(.streams | length) > 0 and
     (((.streams[0].duration | tonumber?) // (.format.duration | tonumber?)) as $d | $d >= $low and $d <= $high)' \
    <<<"$probe" >/dev/null || {
      echo 'FATAL audio reference needs an audio stream within the configured duration limits' >&2; return 65;
    }
}

h3_reference_encode_audio() {
  local path="$1" index="$2" sr channels cap
  local ffmpeg="$(jq_get minimax_h3.references.audio_encoder.ffmpeg)"
  sr="$(jq_get minimax_h3.references.audio_encoder.sampling_rate)"
  channels="$(jq_get minimax_h3.references.audio_encoder.channels)"
  cap=$((FRAMES * sr / FPS))
  h3_reference_audio_preflight "$path" || return
  # Same host recipe as the proven server: stereo 32kHz PCM16, then read as
  # normalized F32. A server-staged PCM16 WAV passes through unchanged.
  "$ffmpeg" -nostdin -n -hide_banner -loglevel error -threads 1 -i "$path" \
    -map 0:a:0 -vn -acodec pcm_s16le -ar "$sr" -ac "$channels" "reference-$index-audio.wav" || return
  "$ffmpeg" -nostdin -n -hide_banner -loglevel error -threads 1 -i "reference-$index-audio.wav" \
    -map 0:a:0 -vn -af "atrim=end_sample=$cap" -acodec pcm_f32le -f f32le "reference-$index-audio.f32" || return
  "${GPU[@]}" "$(jq_get minimax_h3.references.audio_encoder.tool)" \
    --config "$(jq_get minimax_h3.references.audio_encoder.config)" \
    --checkpoint "$(jq_get minimax_h3.references.audio_encoder.checkpoint)" \
    --input-f32 "reference-$index-audio.f32" --channels "$channels" --max-samples "$cap" \
    --output-mean "reference-$index-audio-mean.diftensor" --output-rows "reference-$index-audio-rows.diftensor" \
    --backend cuda --cache-dir "$CACHE" --min-free-mib "$MIN_FREE" || return
  read -r H3_REFERENCE_AUDIO_LATENTS < <(od -An -tu8 -j36 -N8 "reference-$index-audio-mean.diftensor")
  [[ "$H3_REFERENCE_AUDIO_LATENTS" =~ ^[1-9][0-9]*$ ]] || {
    echo 'FATAL invalid encoded audio frame count' >&2; return 65;
  }
}

h3_reference_conditioning() {
  local target_rows="$VIDEO_ROWS" target_audio_rows="$AUDIO_ROWS" total_visual=0 index=0 rw rh gh gw rows count
  local image_index=0 audio_index=0 path kind first_vision=""
  local -a states=() audio_states=() presentations=() visions=()
  local -a reference_layout=()
  local -a previous_draws=()
  : > empty-prompt.txt
  # Validate all audio before any vision/model allocation.
  for index in "${!REFERENCE_PATHS[@]}"; do
    if [[ "${REFERENCE_KINDS[$index]}" == audio ]]; then
      h3_reference_audio_preflight "${REFERENCE_PATHS[$index]}" || return
      [[ -s "$(jq_get minimax_h3.references.audio_encoder.modulation_cache)" ]] || {
        echo 'FATAL reference audio needs its configured four-table modulation cache; run Ref2VA preparation' >&2; return 66;
      }
    fi
  done
  for index in "${!REFERENCE_PATHS[@]}"; do
    path="${REFERENCE_PATHS[$index]}"; kind="${REFERENCE_KINDS[$index]}"
    if [[ "$kind" == audio ]]; then
      audio_index=$((audio_index + 1))
      log "phase=reference-audio index=$index label=$audio_index"
      h3_reference_encode_audio "$path" "$index" || return
      "$B/diftokenize" --processor "$(jq_get minimax_h3.processor)" --prompt "<Audio $audio_index>: " \
        --diftensor-out "reference-$index-audio-label.diftensor" --quiet
      presentations+=(--text-input "reference-$index-audio-label.diftensor")
      audio_states+=(--condition "reference-$index-audio-rows.diftensor")
      reference_layout+=(--reference-geometry "audio:0:0:0:$H3_REFERENCE_AUDIO_LATENTS")
      AUDIO_ROWS=$((AUDIO_ROWS + 2 * H3_REFERENCE_AUDIO_LATENTS))
      continue
    fi
    [[ "$kind" == image ]] || { echo 'FATAL unsupported reference kind' >&2; return 65; }
    log "phase=reference-prepare index=$index"
    "$B/difh3vision" prepare-image --image "$path" \
      --short-edge "$(jq_get minimax_h3.references.image_short_edge)" \
      --output "reference-$index.png" > "reference-$index.json"
    read -r rw rh < <(jq -r '[.width,.height]|@tsv' "reference-$index.json")
    gh=$((rh / $(jq_get minimax_h3.keyframes.vision_patch_size)))
    gw=$((rw / $(jq_get minimax_h3.keyframes.vision_patch_size)))
    count=$((gh * gw / 4)); total_visual=$((total_visual + count))
    "$B/difh3vision" inputs --checkpoint "$CKPT/text_encoder" \
      --processor "$(jq_get minimax_h3.processor)" --image "reference-$index.png" \
      --prompt-file empty-prompt.txt --label-offset "$image_index" --grid-t 1 --grid-h "$gh" --grid-w "$gw" \
      --output "reference-$index-presentation.safetensors"
    "$B/difh3vision" program --grid-t 1 --grid-h "$gh" --grid-w "$gw" \
      --output "reference-$index-vision.difir"
    "$B/difh3vision" bundle --checkpoint "$CKPT/text_encoder" \
      --program "reference-$index-vision.difir" --grid-t 1 --grid-h "$gh" --grid-w "$gw" \
      --sealed-bundle "$(jq_get minimax_h3.keyframes.vision_bundle)" --output "reference-$index-vision.difbind"
    "${GPU[@]}" "$B/difh3vision" run --program "reference-$index-vision.difir" \
      --bundle "reference-$index-vision.difbind" --inputs "reference-$index-presentation.safetensors" \
      --grid-t 1 --grid-h "$gh" --grid-w "$gw" --output "reference-$index-vision.safetensors" \
      --backend cuda --cache-dir "$CACHE" --min-free-mib "$MIN_FREE" \
      --streamed-stage-threads "$(jq_get minimax_h3.policy.streamed_stage_threads)" --report "reference-$index-vision.json"
    presentations+=(--vision-input "reference-$index-presentation.safetensors")
    visions+=(--vision-input "reference-$index-vision.safetensors")
    [[ -n "$first_vision" ]] || first_vision="reference-$index-vision.safetensors"
    log "phase=reference-encode index=$index canvas=${rw}x${rh}"
    "${GPU[@]}" "$B/difh3encode" --backend cuda \
      --program "$(jq_get minimax_h3.keyframes.encoder_program)" \
      --weight-bundle "$(jq_get minimax_h3.keyframes.encoder_bundle)" --image "reference-$index.png" \
      --pixels-id "$(jq_get minimax_h3.keyframes.encoder_pixels_id)" --moments-id "$(jq_get minimax_h3.keyframes.encoder_moments_id)" \
      --output-moments "reference-$index-moments.diftensor" --output-latent "reference-$index-latent.diftensor" \
      --output-rows "reference-$index-rows.diftensor" --tile-size "$(jq_get minimax_h3.decoder.tile_size)" \
      --tile-overlap "$(jq_get minimax_h3.decoder.tile_overlap)" --posterior-seed "$(jq_get minimax_h3.references.posterior_seed)" \
      --cache-dir "$CACHE" --min-free-mib "$MIN_FREE"
    "$B/difh3noise" --rng torch-cpu --layout h3-video --latent-frames 1 \
      --latent-height "$gh" --latent-width "$gw" --seed "$SEED" "${previous_draws[@]}" --output "reference-$index-noise.diftensor"
    previous_draws+=(--skip-normal-draw "$((24 * gh * gw))")
    states+=(--condition "reference-$index-rows.diftensor" --condition-noise "reference-$index-noise.diftensor")
    reference_layout+=(--reference-geometry "image:1:$gh:$gw:0")
    rows=$((gh * gw / 4)); VIDEO_ROWS=$((VIDEO_ROWS + rows))
    image_index=$((image_index + 1))
  done
  # Source presentation emits each label separately, then the prompt verbatim.
  "$B/diftokenize" --processor "$(jq_get minimax_h3.processor)" --prompt-file prompt.txt \
    --diftensor-out reference-prompt.diftensor --quiet
  presentations+=(--text-input reference-prompt.diftensor)
  "$B/difh3vision" combine-inputs "${presentations[@]}" --output presentation.safetensors \
    --ids-out ids.diftensor --tags-out token-tags.diftensor
  if (( image_index == 1 )); then
    cp "$first_vision" vision.safetensors
  else
    "$B/difh3vision" combine "${visions[@]}" --output vision.safetensors
  fi
  read -r T < <(od -An -tu8 -j20 -N8 ids.diftensor)
  [[ "$T" =~ ^[1-9][0-9]*$ ]] || { echo 'FATAL invalid reference text count' >&2; return 65; }
  "$B/difcondition" program --checkpoint "$CKPT" --sequence "$T" --vision-tokens "$total_visual" --output "cond-s$T.difir" > prep.log
  "$B/difcondition" bundle --checkpoint "$CKPT" --program "cond-s$T.difir" --vision-tokens "$total_visual" \
    --output "cond-s$T.difbind" --sealed-bundle "$(jq_get minimax_h3.conditioner_bundle)" >> prep.log
  log "phase=conditioning task=ref2va tokens=$T vision_tokens=$total_visual"
  "${GPU[@]}" "$B/difcondition" run --program "cond-s$T.difir" --bundle "cond-s$T.difbind" \
    --vision-inputs presentation.safetensors --vision-outputs vision.safetensors --vision-tokens "$total_visual" \
    --output conditioning.diftensor --cache-dir "$CACHE" --min-free-mib "$MIN_FREE" \
    --streamed-stage-threads "$(jq_get minimax_h3.policy.streamed_stage_threads)" --report conditioner.json
  "$B/difh3noise" --rng torch-cpu --layout h3-video --latent-frames "$LAT_T" \
    --latent-height "$LAT_H" --latent-width "$LAT_W" --seed "$SEED" "${previous_draws[@]}" --output target-video-noise.diftensor
  previous_draws+=(--skip-normal-draw "$((target_rows * 96))")
  # Creator draws audio directly as channel-major [2*A,32] rows, not a
  # channel-first VAE latent which would transpose the same random values.
  local target_audio_path=audio-noise.diftensor
  if (( audio_index )); then target_audio_path=target-audio-noise.diftensor; fi
  "$B/difh3noise" --rng torch-cpu --layout flat --rows "$target_audio_rows" --cols 32 \
    --seed "$SEED" "${previous_draws[@]}" --output "$target_audio_path"
  if (( audio_index )); then
    "$B/difh3state" --clean-conditions "${audio_states[@]}" --target-noise "$target_audio_path" --output audio-noise.diftensor
  fi
  "$B/difh3state" "${states[@]}" --target-noise target-video-noise.diftensor \
    --condition-timestep "$(jq_get minimax_h3.references.condition_timestep)" --output video-noise.diftensor
  MODCACHE="$(jq_get minimax_h3.modulation_cache)"
  TIMESTEP_TABLES="$(jq_get minimax_h3.profile.timestep_tables)"
  if (( audio_index )); then
    MODCACHE="$(jq_get minimax_h3.references.audio_encoder.modulation_cache)"
    TIMESTEP_TABLES="$(jq_get minimax_h3.references.audio_encoder.timestep_tables)"
  fi
  TEXT_ARGS=(--text-tags token-tags.diftensor)
  LAYOUT_ARGS=("${reference_layout[@]}")
}
