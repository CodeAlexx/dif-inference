# P3 gate — MiniMax-H3 T2VA through the product surfaces (RTX 5080 16 GB)

Engine: Diffusion Compiler `build-5080-release` tools, as-is. Chain per request:
diftokenize → difcondition program+rebind+run (Qwen3-VL text tower) → difc
make-h3-denoiser + difweights rebind → difh3noise ×2 → difh3infer (ConvRot INT8
projections, 28 resident layers, exact cuDNN attention, 20 schedule points / 19
evaluations, sealed modcache) → difvaedecode (tiled) → difaudiodecode → difh3media
(libx264 + AAC). Runner: `scripts/h3_compiler_runner.sh`, installed as
`output/bin/minimax_h3_serenity_runtime` (the server's H3 runner protocol).
Geometry admitted: 832x480, 124 frames @ 24 fps (5.17 s). Others fail loud.

| run | surface | prompt (mine) | seed | wall | result |
|---|---|---|---|---|---|
| run-1 | runner CLI (cold + decode) | fisherman mending a net at dawn | 4242 | 179 s + 27 s | MP4 832x480x124 + AAC 32 kHz stereo; frames at 2.5 s / 4.5 s viewed: on-prompt, temporally consistent; audio non-silent (per-second RMS 152/139/129/84/86) |
| video-0016 | POST /v1/video | red tram, rainy street | 777 | failed | server's Mojo INT8 resident-cache build step invoked the runner with an unsupported request — fixed: runner answers `--prepare-runtime-cache` (ConvRot pack present), server skips the build when the pack exists |
| video-0017 | POST /v1/video | red tram, rainy street | 777 | 171 s | done; progress streamed 1..19 via status.json; frame at 3 s viewed: on-prompt |
| video-0018 | Generate tab click ("Generate H3 Video + Audio") | hiker on a snowy ridge at golden hour | 9001 | 178 s | done; UI showed step progress, video in preview + Current Batch + History; frame at 2.5 s viewed: on-prompt |

Also: the Generate tab now defaults H3 to the server's first supported profile
(the compiler geometry), the runtime label reads "MiniMax-H3 · Diffusion
Compiler · ready", and the topbar carries the live GPU readout (/v1/gpu).

Not covered (fail loud today): I2VA / FL2VA keyframes, Ref2VA, continuation
(motion context), other resolutions/durations, step counts other than 20, the
`high` step cache. Each needs its own sealed VAE/audio/modcache programs or a
compiler-side feature; the server's request contract already carries them.
