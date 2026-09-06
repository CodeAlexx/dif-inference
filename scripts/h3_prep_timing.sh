#!/usr/bin/env bash
# Measure per-prompt H3 program preparation cost (the arbitrary-prompt tax):
# tokenize -> conditioner program+bundle -> denoiser program+bundle for the
# 832x480x124 T2VA geometry (VIDEO 14430, AUDIO 414 rows; 2 timestep tables).
set -euo pipefail
B=/home/alex/diffusion-compiler/build-5080-release
CKPT=/home/alex/.serenity/models/checkpoints/MiniMax-H3/FL2VA
OUT="${1:?outdir}"; PROMPT="${2:?prompt file}"
mkdir -p "$OUT"
t() { local n=$1; shift; local s=$(date +%s.%N); "$@" > "$OUT/$n.log" 2>&1; local e=$(date +%s.%N); printf "%-28s %8.2f s\n" "$n" "$(echo "$e - $s" | bc)"; }
t tokenize "$B/diftokenize" --processor "$CKPT/processor" --prompt-file "$PROMPT" --strip-trailing-newline --diftensor-out "$OUT/ids.diftensor"
T=$(python3 -c "
import struct,math;b=open('$OUT/ids.diftensor','rb').read();r=struct.unpack_from('<I',b,16)[0];d=struct.unpack_from('<%dQ'%r,b,20);print(math.prod(d))")
echo "text_tokens=$T"
t cond-program "$B/difcondition" program --checkpoint "$CKPT" --sequence "$T" --output "$OUT/cond-s$T.difir"
t cond-bundle "$B/difcondition" bundle --checkpoint "$CKPT" --program "$OUT/cond-s$T.difir" --output "$OUT/cond-s$T.difbind"
t denoiser-program "$B/difc" make-h3-denoiser "$OUT/h3-832x480x124-t$T-tables2-exact-cudnn.difir" 14430 414 "$T" 2 streamed cudnn
t denoiser-bundle "$B/difweights" make-h3-denoiser-bundle "$CKPT/transformer/model.safetensors.index.json" "$OUT/h3-832x480x124-t$T-tables2-exact-cudnn.difir" "$OUT/h3-832x480x124-t$T-tables2-exact-cudnn.difbind"
ls -la "$OUT" | grep -E "difir|difbind"
