# Native H3 runnability — 2026-09-06

Scope: H3 in `dif-inference`, using the existing Diffusion Compiler tools and
shared native runtime. Krea was explicitly excluded. No MojoDiffusion or
SerenityFlow source was edited, no model was downloaded, and no commit/push was
made. This is not completion of the request for **all H3 features**.

## Restored path

- `config/difc.json` selects the real existing sealed conditioner, denoiser,
  video/audio decoders, modulation cache, and keyframe programs. Preparation
  uses `scripts/prepare-h3.sh` and native `difmodcache`; no neural Python runtime.
- T2VA, I2VA, L2VA and FL2VA use one request runner and the same decode/mux path.
  Keyframe inputs use native Qwen3-VL vision/text conditioning, native image VAE
  encoding, tagged packed rows, and the appropriate first/last anchor layout.
- `scripts/h3-keyframes.sh` is frontend orchestration, not a new inference engine.
- Image inputs must already match the sealed 832×480 canvas. The server snapshots
  them as RGB PNG without resizing; tests check exact pixel preservation.
  Creator-compatible arbitrary-image resizing remains unimplemented.
- Current profile: 832×480, 124 frames, 24 FPS, 20 schedule points / 19 model
  evaluations, 50 blocks, Euler, exact cuDNN attention. ConvRot INT8 projections
  are approximate. `int8-fast` is a compatibility alias for the same INT8 route,
  not a separate W8A8 path. Text/vision conditioning is BF16.
- H3 Studio consumes the server's configured resolutions, quantization labels,
  and cache availability. Numeric edits are saved before a panel redraw can
  discard them. Generate and Studio both use `/v1/video`.
- Native admission rejects unsupported task/cache/attention choices, unconsumed
  keyframes or ordered references, LoRA, mismatched output duration, and invalid
  keyframe dimensions before generation. The CLI also rejects unknown flags.

## Evidence

Raw logs and source prompts are preserved in
`/tmp/dif-h3-runnable-20260906.NxCoWi`. Outputs are in `output/run`.

Hardware: NVIDIA RTX 3090 Ti, 24 GB, shared desktop. All neural stages run under
the existing 24 GiB host cap, unlimited MemoryHigh, 2 GiB swap cap, 16 GiB desktop
reserve and `/tmp/dc-gpu.lock`. GPU peak memory was not independently sampled.

### T2VA — video-0006

- Real structured hiker/mountain prompt, seed 9001; no synthesized embeddings.
- Actual Generate-button run; all 19 denoiser evaluations, video decode, audio
  decode and mux succeeded. UI timer: **317.2 s PTF**. Warm filesystem/module
  caches after a bounded smoke; fresh processes, no persistent resident model.
- H.264 832×480, 124 frames at 24 FPS; AAC 32 kHz stereo; 5.167 s MP4.
- Viewed frames 0, 62 and 123: coherent mountain scene and hiker, no flat/white
  output. This is sampled visual inspection, not full temporal/audio review.
- Generation cgroup peak 12,786,880,512 bytes; decode peak 6,616,989,696 bytes;
  neither phase recorded high/max/OOM events.
- MP4 SHA256: `7bffd6fffd1c9b1cff632bb9eb552a3086468871d157c8f358622a55504b99cb`.

### FL2VA — video-0007

- Actual H3 Studio Queue-take run with real first/last images extracted from
  video-0006, exact structured alignment prompt, seed 9001.
- Native presentation: 1,012 text/multimodal tokens, 780 visual tokens; two real
  image encodes. All 19 evaluations succeeded; output is H.264 832×480, 124 frames
  at 24 FPS, AAC 32 kHz stereo, 5.167 s.
- Viewed frames 0, 62 and 123. Opening and ending closely follow supplied
  keyframes, with a coherent intermediate hiker/mountain frame.
- Request-file creation: 10:17:15.053729413 PDT; MP4 completion mtime:
  10:22:10.502183883 PDT. Difference **295.45 s server-file boundary**, NOT an
  exact Generate-button PTF: confirmation timing was not independently captured.
- Generation cgroup peak 4,981,989,376 bytes; high/max/OOM events all zero.
- MP4 SHA256: `212f91c3a9d4f936d75142460288c36be6b4f53480dd75c83053792d11c6d880`.

### I2VA — video-0008

- Full API request using the first-frame instruction, one image, seed 9001.
  All 19 evaluations and native video/audio decode/mux passed.
- H.264 832×480, 124 frames / 24 FPS; AAC 32 kHz stereo, 5.167 s.
- Viewed frames 0, 62 and 123: opening follows the supplied frame; hiker and
  snowy mountain remain coherent as the composition develops.
- Request-file → saved-MP4 boundary: 10:25:16.441022142 →
  10:30:16.819513943 PDT, **300.38 s**, not a button-timed PTF.
- Generation host peak 5,701,939,200 bytes; decode peak 6,614,794,240 bytes;
  high/max/OOM events all zero.
- MP4 SHA256: `7e8e0985ea09ceeafffcc51772cabe0182323f381f0b907af680dd10bf712ddd`.
- The native `H3_DENOISE` line calls all nonempty base-anchor layouts `fl2va`;
  this run used `--keyframes first`, not two input images. Server request/result
  retain the specific `i2va` task.

### L2VA — video-0009

- Full API request using the last-frame instruction, one ending image, seed 9001.
  All 19 evaluations and native video/audio decode/mux passed.
- H.264 832×480, 124 frames / 24 FPS; AAC 32 kHz stereo, 5.167 s.
- Viewed frames 0, 62 and 123: coherent approach toward the supplied ending
  composition. The final frame closely follows the requested hiker/mountain image.
- Request-file → saved-MP4 boundary: 10:31:10.598285541 →
  10:35:47.181582061 PDT, **276.58 s**, not a button-timed PTF.
- Generation host peak 3,332,816,896 bytes; decode peak 6,612,738,048 bytes;
  high/max/OOM events all zero.
- MP4 SHA256: `984789b29ad4ea3d651c74866f80521de05f9b083e51873ae1a0716dd7a360ed`.

These are runnability/decoded-output gates, not matched speed comparisons or
fresh creator-oracle numerical parity. Do not compare the two timings as a
speedup. CPU inspection/tests overlapped parts of the keyframe run.

### Preparation and tests

- Native T2VA cache: 50 blocks, 38 distinct times, two tables, 20 schedule points;
  SHA256 `83dae1bd3a5838bd72aac6295e05a5bf2995300fb50532e6a51ad1edbd6b0f9e`.
  Builder reached its 24 GiB host ceiling with 249 max events, no OOM. This build
  was **not pressure-free**. Generation did not repeat that pressure.
- Separate T2VA and FL2VA one-evaluation native smoke runs passed; those are not
  substitutes for the full outputs above.
- Final targeted Rust run: 9 config tests and 194 server tests passed. One
  pre-existing LTX creator-cache test was filtered because its Creator checkout
  is absent; the unfiltered run confirmed that separate failure.
- Request defaults are normalized from JSON before native H3 admission. Explicit
  values, including malformed/null inputs, are preserved for validation rather
  than silently replaced.
- Release config/server build passed. H3 shell syntax checks, JavaScript syntax
  checks, H3 project/endless contract test, and `git diff --check` passed.
- Existing Krea runner remains byte-identical to the start of this H3 task:
  SHA256 `6b892f509866ec1ab64faf262348779065b85cf8be594fbbc37c5c3ceb5d3ece`.
  Its dirty-tree changes predate the user's exclusion and were preserved.

## Still not delivered

- Separate Ref2VA checkpoint deployment and image/video/audio reference gates.
  Local Ref2VA weights exist, but an older artifact under
  `diffusion-compiler/artifacts/h3-flref-convrot-2026-09-01/ref-real-dims/`
  explicitly binds FL2VA shard paths. Its name is not Ref2VA model evidence.
  Both transformer **index JSON files have the same hash**, so an index hash
  alone is not checkpoint-content identity; inspect sealed shard paths/hashes.
- Native arbitrary reference-video/audio ingestion/encoding, LoRA, ControlNet,
  motion-context continuation, arbitrary guide/mask features, and the distinct
  Mojo middle-block cache have not been ported/accepted here.
- Arbitrary geometry, durations, keyframe resizing and output trimming/resampling
  are not admitted by this sealed profile.
- CK/Sage alternatives and cache presets need their own native routing/quality
  gates. Existing Mojo performance numbers are not compiler acceptance evidence.
- BF16 is wired but not full-generation tested in this task. Perceptual audio
  quality and full temporal inspection remain open even for the successful MP4s.

Use `bash scripts/prepare-h3.sh` for explicit cache readiness. Use H3 Studio's
opening/ending frame fields for the native keyframe modes, selecting the
configured resolution, 5.167 seconds, 20 steps and cuDNN/Exact. Ref2VA and native
continuation remain unavailable. Fastloader remains opt-in, not default.
