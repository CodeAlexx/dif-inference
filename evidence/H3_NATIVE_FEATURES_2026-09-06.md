# Native H3 feature integration — in progress

Scope: Diffusion Compiler plus `dif-inference` deployment. MojoDiffusion and
SerenityFlow are source references, not fallback inference processes. Krea is
excluded. ASM fastload remains opt-in (`DIF_FASTLOAD=1`), not the default.

This is a progress record, **not an all-features or decoded-quality admission**.
Existing Base T2VA/I2VA/L2VA/FL2VA output gates are recorded separately in
`H3_NATIVE_RUNNABILITY_2026-09-06.md`; they do not validate these new features.

Source availability clarification: MojoDiffusion already contains the native
GPU audio encoder, Ref2VA video/audio preparation, continuation, LoRA, residual
cache and control preprocessing. Remaining gaps below are **compiler ports,
integration or verification**, not claims that those source implementations
are absent. Actual source-to-compiler mapping:
`/home/alex/diffusion-compiler-docs/docs/models/h3/H3_EXISTING_SOURCE_PORT_MAP_2026-09-06.md`.

## Implemented and measured boundaries

- Native audio encoder is now ported into Compiler as `difaudioencode`, with
  the existing Mojo operation order, F32 math/BF16 causal attention boundary,
  posterior mean and normalized reference rows. The real Ref2VA AudioVAE ran
  on RTX 3090 Ti with 3201 stereo samples: mean `[2,32,5]`, rows `[10,32]`,
  nonfinite=0, observed host peak 883,732,480 bytes under the 4 GiB guard.
  Integration contract tests pass. This is an encoder execution smoke, not
  a new source-parity campaign or full reference-audio video-generation gate.
  Config and checkpoint/tool paths live in JSON. Evidence:
  `/tmp/h3-audio-port.ndIFRn/run.log`; detailed source/receipt record:
  `/home/alex/diffusion-compiler-docs/docs/models/h3/H3_NATIVE_AUDIO_ENCODER_PORT_2026-09-06.md`.
- Ordered image/audio Ref2VA requests are now wired through the server and
  compiler runner, including reference/reuse/voice-timbre prompt instructions.
  Source-faithful audio labels are independently tokenized in request order;
  clean audio rows are prefixed without condition-noise draws. Nine native CLI
  integration checks and all 12 config tests pass. The real runner audio helper
  resampled a two-second mono 16 kHz clip to stereo 32 kHz and encoded it to
  `[2,32,80]` mean / `[160,32]` rows, nonfinite=0, observed host peak 888,758,272
  bytes under the 4 GiB guard. Mixed conditioning selects its new four-table
  modulation cache; all 50 blocks prepared with observed peak 1,853,534,208
  bytes. Image-only keeps its three-table cache. Server staging tests cover
  interleaved references and all audio roles. Evidence:
  `/tmp/h3-reference-audio.bpUQPn/`. Full mixed-reference MP4 remains unverified.
- LoRA canonical/legacy H3 mapping and immutable ConvRot activation overlays:
  CPU tests and genuine tiny CUDA BF16/ConvRot projections pass byte-for-byte,
  including fractional scale 0.7, mixed resident/streamed weights and repeated
  prepared execution. The overlay does not requantize the base checkpoint.
- Middle-block residual caching: distinct from whole-model EasyCache. Source
  policy uses front/back bands of 8 blocks, 4 warmups, 3 exact tail evaluations,
  separate mixed-sequence/audio probes and group-32 signed INT8 residuals.
  Full 50-block small CPU graph partition/reassembly passes. Generic tiny CUDA
  Refresh/Reuse/Reuse/Exact sequences pass byte-for-byte against the typed CPU
  oracle in both serial and overlapped execution. No H3 speed claim yet.
- ControlNet: 5 native side blocks, with residual injection after base blocks
  0/10/20/30/40. Actual 74-tensor checkpoint census (6,806,835,712 bytes) passes.
  Temporal/spatial/posterior and row-packing CPU fixtures agree exactly at all
  6 tested boundaries with extracted source methods. This substitutes fixture
  callbacks at the learned encoder boundary; it is not a neural forward gate.
- Prepared-control shell checks use real ffmpeg/ffprobe: short clips, still
  images, unsupported Canny/source/mask inputs and nonfinite controls fail
  before GPU execution. Zero strength is an explicit no-op. Ordinary C24
  guide latents patchify to 96 features and zero-pad to 196, matching creator
  pure-generation layout. C49 guide/visibility/masked-source is a separate,
  not-yet-implemented input mode, not an implicit all-visible mask.
- Motion context: native compact SafeTensors load/save, prefix layout, fixed
  Euler condition masks and 5/22/39-frame tails pass CPU mechanics. The Mojo
  extension uses literal F32 `0.001` for condition noise; it is intentionally
  kept distinct from generic `1.0F - 0.999F` reference augmentation.
- Ref2VA image preparation: 8 Pillow/native Lanczos cases are byte-exact.
  Request RNG order now reproduces condition 1, condition 2, target video,
  then direct `[2*A,32]` audio draws. Fifteen CPU PyTorch oracle cases pass,
  including non-multiple-of-16 draw boundaries. VAE posterior sampling uses
  an independent fixed seed 42, not the request seed.
  Source inspection found the same restart/seed/audio-layout errors in the
  existing native keyframe shell path; these are corrected there too. Prior
  keyframe videos remain valid runnability evidence, not creator RNG parity;
  fixed-seed bytes change with this correctness fix and require a new output gate.

## Source identities and deployment

- H3 creator checkout: `minimax_h3_ref/diffusers-src` at
  `e1b518dfd5e390e7ba09a79a1d39fe1c6cb52dc1`.
- ControlNet creator VideoX-Fun:
  `968f0e2192ba4c7a12868bf36d73260d135424ca`.
- Separate `config/h3-ref2va.json` resolves Ref2VA checkpoint-dependent paths
  after inheritance; it does not substitute Base's structurally identical
  transformer index. Sealed receipts validate exact shard paths and sizes.
- Existing Ref2VA conditioner and vision receipts were inspected and reused;
  their actual paths name Ref2VA's 14-shard text encoder. Denoiser and derived
  INT8/modulation artifacts are prepared explicitly from Ref2VA weights.
- Original Base/Ref2VA video VAE and audio VAE files were fully compared and
  were byte-identical, permitting reuse of the existing shared VAE bundles.
- Continuation delivery remains the configured 124 frames. With overlap
  5/22/39, aligned target timelines are 141/158/175 frames respectively; the
  fixed context is a separate prefix. Video/audio decode must trim that
  overlap and cap delivery, and tail saving must use the delivered endpoint.
- Audio decoder geometry also changes generated block-average constants.
  Generic bundle rebinding correctly rejected their shape mismatch. The new
  `difimport rebind-audio-bundle` path now regenerates only generated constants
  and retains learned-weight receipts. Native CTest passes and the actual
  continuation helper prepares all 5/22/39-frame overlap geometries using the
  sealed decoder. It reuses 779 learned bindings (261,416,256 bytes) and writes
  nine generated constants, seven of whose shapes change. This is preparation,
  not a decoded continuation/audio gate.

## Preserved negative evidence

The first LoRA GPU attempt exposed a real distinction between Internal
physical Q/K/V weights and semantic Constant replacements. Subsequent failures
also caught two errors in the handcrafted test fixture: Input|Constant roles
where the production model uses Constant-only, and a Walsh-style test pack
that did not match this project's actual ConvRot reflection transform. The
tests were corrected against source; tolerances were not relaxed.

The initial Ref2VA preparation wrapper used an environment reentry marker
which the clean systemd environment did not forward, causing recursive wrapper
launches. Only the task-owned preparation units were stopped. Reentry now uses
an explicit CLI flag. Failure logs and abandoned preparation stages remain;
the second run was stopped cleanly, but the third run caused a session-wide
systemd-oomd incident at 11:41:05 PDT during modulation-cache preparation.
The child peaked at 11.7 GiB: a 24 GiB child cap alone did not protect the
ancestor session from sustained pressure. This supersedes any earlier claim
that the corrected reentry wrapper made the preparation safe.

Recovery added serial heavy-job admission, host/ancestor/child pressure
monitoring and exact owned-service cancellation. Thirteen guard tests pass.
An actual full builder was cancelled at 90% of its 4 GiB ceiling while the
desktop survived; most of its footprint was retained checkpoint file cache.
The one-time builder now releases pages after their final host use. All 50
Ref2VA blocks subsequently completed at 1,483,657,216 observed peak bytes,
without sampled pressure or high/max/OOM events. One-block before/after files
are byte-identical, and full-cache block 0/final also match the old reference.
The validated full modulation cache is installed without overwriting anything.
Default Ref2VA INT8 preflight still refuses generation because its ConvRot
cache is missing; this is independent of the completed audio modulation cache.

Detailed incident, guard policy, measured limits and preserved logs:
`/home/alex/diffusion-compiler-docs/docs/runtime/H3_PREPARATION_SESSION_OOM_2026-09-06.md`.

## Gates still open

- Full learned ControlNet/LoRA output gates and viewed decoded artifacts.
- Full native Ref2VA prompt-to-saved-video run using the real Ref2VA checkpoint.
- Real continuation decode/mux, seam inspection and soundtrack inspection.
- Matched exact-versus-middle-cache H3 quality and continuous PTF measurement.
- Reference-video Ref2VA request ingestion remains to be wired: genuine paired
  frames, timestamps and video tokens. Ordered image/audio requests are now
  wired; video and audio-only requests remain explicitly rejected.
- Combined ControlNet with reference/keyframe/motion layouts or residual
  caching; these combinations currently fail closed.
  The Flow source itself rejects reference/keyframe conditioning in its
  ControlNet media route; full support must preserve applicable source
  restrictions rather than invent unsupported combinations.
- Optional reference VAE behavior from ComfyUI PR #16065 is deferred at the
  user's request until the current feature work, not silently treated as done.

PTF is one continuous prompt/Generate-to-finished-saved-MP4 wall clock. Loading,
conditioning, denoising, both decoders and mux belong inside it. CPU mechanics,
tiny GPU gates, one-time preparation and sums of separately run stages are not
PTF claims. Record cache/process/residency and any UI/queue exclusions with a run.

## Local evidence

- Integration lab: `/tmp/dif-h3-features-20260906.ovJ1c5`.
- LoRA passing GPU log: `/tmp/dif-h3-lora-cuda-retry3-20260906.log`;
  peak host memory 326,062,080 bytes, no memory-pressure/OOM events.
- Residual-cache GPU log: `/tmp/dif-residual-cache-cuda-20260906.log`;
  peak host memory 114,622,464 bytes, no memory-pressure/OOM events.
- Control/source CPU lab: `/tmp/h3-controls-cpu.KFclCA`.
- Ref2VA RNG: `noise-sequence-2.log`; old malformed test-reader attempt is
  preserved separately as `noise-sequence.log`.
- Guard tests: `runtime-guard-final-tests.log`; rejected and successful full
  modulation runs: `ref2va-modcache-full-guarded.log` and
  `ref2va-modcache-full-evict.log` in the integration lab.
- Actual continuation audio preparation:
  `/home/alex/dif-inference/output/h3-motion-audio-gate.Ak7oV7`.
- Latest config/server test logs record their exact test counts; these are
  admission/transport mechanics, not generation claims.

No feature server deployment or new decoded gate is implied by this document.
# Ref2VA image + audio: saved output, 2026-09-06 14:41 PDT

The actual `/v1/video` request `output/ref2va-audio-lab/video-0003` completed
all 19 BF16/cuDNN denoise evaluations. Its original API status remains failed
at decode; it has not been relabeled successful. Decode-only recovery through
`scripts/h3_compiler_runner.sh` now produced:

`output/ref2va-audio-lab/video-0003-existing-rgb.EJcG4q/video.mp4`

- H.264, 832x480, 124 frames at 24 FPS, 5.166667 seconds; stereo 32 kHz AAC.
- SHA256 `0115b65107b88ede21601546f4bb49f3ab279b1ad753d87fa0614107f036b69c`.
- Same saved denoise tensors, seed 9001, ordered audio then image reference.
- Existing native `difvaedecode --output-rgb` and native audio decoder reused;
  shell/config wiring only in the final repair. Continuation keeps its existing
  tensor/trim delivery route, which was not exercised by this recovery.
- Audio handoff selects only generated rows: native inference writes reference
  rows before target rows, while the sealed audio decoder accepts target rows.
  The runner copies that tail without floating-point arithmetic, following
  `src/frontend/h3_latents.cpp:208` and `tools/difh3infer.cpp:1883`.
- Guard peak 3,646,914,560 bytes under the unchanged 4 GiB cap; zero high/max/OOM
  events. Receipt: `output/runtime-guard/serenity-runtime-memory-20260906-144018-2608113.jsonl`.
- Five sampled frames were inspected: red-jacket hiker moves on a snowy ridge;
  no blank output or obvious corruption. Audio decoder reports nonfinite=0;
  the encoded audio stream is verified, but full listening review is not claimed.

Raw log: `/tmp/h3-reference-audio.bpUQPn/existing-rgb-decode.log`. Earlier failed
decodes remain intact. This is a recovered app request, not a fresh uninterrupted
API success or a PTF benchmark. Earlier shared-compiler loader/tensor/decoder
lifetime edits remain uncommitted and are not fully regression-tested. Krea's
runner hash remains unchanged; ASM fastload remains off.
