#!/usr/bin/env bash
# MiniMax-H3 request runner backed by the Diffusion Compiler's native process
# chain. Speaks the serenity-server H3 runner protocol (video/minimax_h3.rs):
#
#   cold:   runner <prompt> <out_dir> <steps> <seed> <blocks> --width=W --height=H
#                  --frames=F --output-frames=N --fps=24 --output-fps=24 --quant=Q
#                  --resident-blocks=R ... --defer-video-decode
#           -> conditioning + denoise; leaves video-latent/audio-rows in out_dir;
#              progress lines on stdout (the server tails runner.log).
#   decode: runner decode <out_dir> <steps> <seed> <blocks> decode_video_only --width=...
#           -> video VAE decode + audio decode + mux -> out_dir/video.mp4, result.json
#
# Everything neural runs in the compiler's tools (no Mojo, no PyTorch). Programs
# are generated per request and their bundles REBOUND from one sealed base
# bundle per geometry (0.07 s, byte-identical to a fresh seal — see
# evidence/p3/P3_H3_PREP_COST_2026-09-05.md).
set -euo pipefail

ROOT="${SERENITY_REPO_ROOT:-$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)}"
CFG="${DIFC_CONFIG:-$ROOT/config/difc.json}"
jq_get() { python3 -c "import json,sys; d=json.load(open('$CFG')); v=d
for k in sys.argv[1].split('.'): v=v[k]
print(v)" "$1"; }
B="$(jq_get compiler_build)"
CKPT="$(jq_get minimax_h3.checkpoint)"
FIX="$(jq_get minimax_h3.fixture_dir)"
CONVROT="$(jq_get minimax_h3.convrot_int8)"
CACHE="$(jq_get minimax_h3.cache_dir)"
RESIDENT_LAYERS_DEFAULT="$(jq_get minimax_h3.resident_layers)"

# The server's INT8 "runtime-cache build" is a Mojo-era step (resident W8A8 /
# groupwise stores). The compiler chain's INT8 route is the sealed ConvRot pack,
# already on disk; answer the build request truthfully and return.
for a in "$@"; do
  if [[ "$a" == "--prepare-runtime-cache" ]]; then
    echo "[difc-h3] runtime cache: compiler ConvRot INT8 pack is the resident store ($CONVROT); nothing to build"
    [[ -s "$CONVROT" ]] || { echo "FATAL ConvRot pack missing: $CONVROT" >&2; exit 66; }
    exit 0
  fi
done
mode=cold
if [[ "${1:-}" == "decode" ]]; then mode=decode; shift; PROMPT=""; else PROMPT="${1:?prompt}"; shift; fi
OUT="${1:?out_dir}"; STEPS="${2:?steps}"; SEED="${3:?seed}"; BLOCKS="${4:-50}"; shift 4 || true
[[ "${1:-}" == "decode_video_only" ]] && shift
W=832; H=480; FRAMES=124; OUT_FRAMES=124; FPS=24; OUT_FPS=24; QUANT=int8; RESIDENT="$RESIDENT_LAYERS_DEFAULT"
for a in "$@"; do
  case "$a" in
    --width=*) W="${a#*=}";; --height=*) H="${a#*=}";; --frames=*) FRAMES="${a#*=}";;
    --output-frames=*) OUT_FRAMES="${a#*=}";; --fps=*) FPS="${a#*=}";; --output-fps=*) OUT_FPS="${a#*=}";;
    --quant=*) QUANT="${a#*=}";; --resident-blocks=*) :;; --motion-context=*) echo "FATAL continuation (motion context) is not implemented in the compiler chain" >&2; exit 65;;
    *) :;;
  esac
done
mkdir -p "$OUT"
cd "$OUT"
log() { echo "[difc-h3] $*"; }

# Geometry (the same rule the server uses): latent grid and packed rows.
LAT_H=$((H / 16)); LAT_W=$((W / 16))
LAT_T=$(( (FRAMES - 5) / 17 * 5 + 2 ))
AUDIO_LATENTS=$(python3 -c "print(round($FRAMES / $FPS * 40))")
ROWS_PER_FRAME=$(( (LAT_H / 2) * (LAT_W / 2) ))
VIDEO_ROWS=$((LAT_T * ROWS_PER_FRAME)); AUDIO_ROWS=$((AUDIO_LATENTS * 2))
log "geometry ${W}x${H}x${FRAMES} latent ${LAT_T}x${LAT_H}x${LAT_W} video_rows=$VIDEO_ROWS audio_latents=$AUDIO_LATENTS audio_rows=$AUDIO_ROWS steps=$STEPS seed=$SEED quant=$QUANT"

# Admitted product geometry: the frozen 832x480x124 contract whose decoder,
# audio and modulation programs are sealed in the fixture. Others fail loud.
if [[ "$W" != 832 || "$H" != 480 || "$FRAMES" != 124 ]]; then
  echo "FATAL geometry ${W}x${H}x${FRAMES} not admitted by the compiler chain yet; admitted: 832x480x124 (5.17 s)" >&2; exit 65
fi
if [[ "$STEPS" != 20 ]]; then
  echo "FATAL steps=$STEPS not admitted: the sealed AdaLN modulation cache is for 20 schedule points (19 evaluations)" >&2; exit 65
fi
MODCACHE="$CKPT/serenity_runtime_cache_v1/modcache_steps_20_blocks_50.safetensors"
if [[ -n "${DIFC_LOCK_HELD:-}" ]]; then GPU=(); else GPU=(flock -w 3600 /tmp/dc-gpu.lock); fi

if [[ "$mode" == cold ]]; then
  printf '%s' "$PROMPT" > prompt.txt
  log "phase=tokenize"
  "$B/diftokenize" --processor "$CKPT/processor" --prompt-file prompt.txt --strip-trailing-newline --diftensor-out ids.diftensor
  T=$(python3 -c "import struct,math;b=open('ids.diftensor','rb').read();r=struct.unpack_from('<I',b,16)[0];print(math.prod(struct.unpack_from('<%dQ'%r,b,20)))")
  log "text_tokens=$T"
  log "phase=prepare conditioner program (sequence $T) + rebind"
  "$B/difcondition" program --checkpoint "$CKPT" --sequence "$T" --output "cond-s$T.difir" > prep.log
  "$B/difcondition" bundle --checkpoint "$CKPT" --program "cond-s$T.difir" --output "cond-s$T.difbind" --sealed-bundle "$FIX/conditioner-s439.difbind" >> prep.log
  log "phase=conditioning"
  "${GPU[@]}" "$B/difcondition" run --program "cond-s$T.difir" --bundle "cond-s$T.difbind" --ids ids.diftensor --output conditioning.diftensor --cache-dir "$CACHE" --report conditioner.json
  log "REAL conditioning: done tokens=$T"
  log "phase=prepare denoiser program + rebind"
  DEN="h3-${W}x${H}x${FRAMES}-t$T-tables2-exact-cudnn"
  "$B/difc" make-h3-denoiser "$DEN.difir" "$VIDEO_ROWS" "$AUDIO_ROWS" "$T" 2 streamed cudnn >> prep.log
  "$B/difweights" rebind-h3-denoiser-bundle "$FIX/h3-832x480x124-t439-tables2-exact-cudnn.difbind" "$CKPT/transformer/model.safetensors.index.json" "$DEN.difir" "$DEN.difbind" >> prep.log
  log "phase=noise"
  "$B/difh3noise" --rows "$VIDEO_ROWS" --cols 96 --seed "$SEED" --output video-noise.diftensor
  "$B/difh3noise" --rows "$AUDIO_ROWS" --cols 32 --seed "$((SEED + 1))" --output audio-noise.diftensor
  log "modcache: HIT $MODCACHE"
  ROUTE=()
  if [[ "$QUANT" == bf16 ]]; then
    log "route=exact-bf16 (no ConvRot; all blocks streamed)"
  else
    ROUTE=(--h3-convrot-int8-checkpoint "$CONVROT" --h3-convrot-int8-resident-layers "$RESIDENT" --h3-int8-mlp-chunk-rows 2048 --h3-int8-cutlass-scaled-all --h3-int8-compact-adaln)
    log "route=convrot-int8 resident_layers=$RESIDENT exact cuDNN attention"
  fi
  log "phase=denoise start"
  # Translate the compiler's H3_STEP lines into the server's progress grammar.
  set +e
  "${GPU[@]}" "$B/difh3infer" --backend cuda --sampler euler \
    --denoiser-program "$DEN.difir" --denoiser-bundle "$DEN.difbind" \
    --all-text-tokens "$T" --text conditioning.diftensor --video video-noise.diftensor --audio audio-noise.diftensor \
    --schedule-points "$STEPS" --latent-t "$LAT_T" --latent-h "$LAT_H" --latent-w "$LAT_W" --audio-latents "$AUDIO_LATENTS" --keyframes none \
    --output-latent video-latent.diftensor --output-audio audio-rows.diftensor \
    "${ROUTE[@]}" --h3-cache-text-refiner \
    --h3-modulation-cache "$MODCACHE" --h3-modulation-source-index "$CKPT/transformer/model.safetensors.index.json" --h3-modulation-steps 20 \
    --lazy-resident-upload --streamed-stage-threads 4 --streamed-keep-pages --denoise-only --profile-pipeline \
    --cache-dir "$CACHE" --min-free-mib 768 2>&1 | while IFS= read -r line; do
      echo "$line"
      if [[ "$line" == H3_STEP\ index=* ]]; then
        idx="${line#H3_STEP index=}"; idx="${idx%% *}"
        echo "[difc-h3] phase=denoise step=$((idx + 1)) total=$((STEPS - 1))"
      fi
    done
  rc=${PIPESTATUS[0]}; set -e
  if [[ $rc -ne 0 || ! -s video-latent.diftensor ]]; then echo "FATAL denoise failed rc=$rc" >&2; exit 70; fi
  log "phase=denoise complete"
  exit 0
fi

# decode
log "phase=decode video (tiled VAE)"
"${GPU[@]}" "$B/difvaedecode" --backend cuda --program "$FIX/decoder-native-tile-l36.difir" --weight-bundle "$FIX/decoder-native-tile-l36.difbind" \
  --input video-latent.diftensor --latent-id 1 --raw-id 1223 --output-raw video-raw.diftensor --output-decoded video-decoded.diftensor \
  --clip-length 17 --token-drop 3 --tile-size 256 --tile-overlap 64 --warmups 0 --iterations 1 --min-free-mib 768 --cache-dir "$CACHE"
log "phase=decode audio"
"${GPU[@]}" "$B/difaudiodecode" --backend cuda --program "$FIX/audio.difir" --weight-bundle "$FIX/audio.difbind" --input audio-rows.diftensor --output-wav audio.wav --cache-dir "$CACHE" --min-free-mib 768
log "phase=mux"
"$B/difh3media" --video video-decoded.diftensor --audio-wav audio.wav --output-dir media --input-fps "$FPS" --output-fps "$OUT_FPS" --encoder libx264
cp media/video.mp4 video.mp4
rm -f video-raw.diftensor video-decoded.diftensor
python3 - "$OUT" "$W" "$H" "$FRAMES" "$OUT_FPS" "$SEED" "$STEPS" "$QUANT" <<'PY'
import json, sys, os
out, w, h, frames, fps, seed, steps, quant = sys.argv[1:]
res = {"state": "done", "artifact_path": os.path.join(out, "video.mp4"), "engine": "diffusion-compiler",
       "width": int(w), "height": int(h), "frames": int(frames), "fps": int(fps), "seed": int(seed), "steps": int(steps), "quant": quant,
       "audio": os.path.join(out, "audio.wav"), "conditioner_report": os.path.join(out, "conditioner.json")}
try:
    res["media"] = json.load(open(os.path.join(out, "media", "result.json")))
except Exception:
    pass
json.dump(res, open(os.path.join(out, "result.json"), "w"), indent=1)
PY
log "done $OUT/video.mp4"
