# SDXL Base 1.0 UI gate

Date 2026-09-06. Requested: add existing SDXL and FLUX.2 Dev compiler pipelines
to dif-inference. User then required smaller Dev (not the removed BF16 model),
suggested checking cache, and explicitly deferred Dev if its encoder could not
be found. Only SDXL was integrated in this turn. No downloads or model deletions.

## Scope and implementation

SDXL Base 1.0 routes through the existing native `difsdxlsample` in the same
Rust compiler worker used for Klein and Krea. Config `sdxl` pins the complete
checkpoint and CLIP tokenizer, with F16 UNet/CLIP and BF16 VAE. No Python or
Mojo production worker. Model identity/checkpoint canonical paths must agree;
UNet-only and unconfigured custom checkpoints are not silently substituted.

UI defaults: 50 steps, CFG 7, Euler / normal discrete DDPM schedule, 1024 square.
Positive/negative prompts, seed, shape, steps and CFG reach the executable.
LoRA, image/reference input, editing, unsupported samplers/schedules, VAE
overrides and unsupported dtype/semantic extra_args fail loudly. Sampler
catalog and capability profile both expose only Euler/normal for SDXL.

Only compiler change: `--report-steps` additionally emits flushed, one-based
`SDXL_NATIVE_STEP step=N/M ms=...` lines on stdout for worker progress. Original
stderr diagnostics retained. No inference, loading, scheduler or kernel math
changed. Fastloader remains off by default.

## Exact inputs and artifacts

- Checkpoint: `/home/alex/.serenity/models/checkpoints/sd_xl_base_1.0.safetensors`.
- Tokenizer: local `stabilityai/stable-diffusion-xl-base-1.0` snapshot
  `462165984030d82259a11f4367a4eed129e94a7b/tokenizer`.
- GPU: RTX 3090 Ti, 24 GiB; one generation at a time under `/tmp/dc-gpu.lock`.
- Host memory capped at 24 GiB, swap 2 GiB, desktop reserve 16 GiB.
- Prompt: `A blue ceramic teapot on a wooden table beside a sunlit window, a small vase of yellow flowers, detailed still life photograph.`
- Negative: `blurry, low quality, text, watermark`.
- Seed 4242, 1024x1024, CFG 7, Euler/normal, one image, style None.

Actual browser Generate clicks: `job-0004` two-step memory/route smoke, then
`job-0005` full 50-step output. The full PNG was inspected in Chrome and directly:
coherent glossy blue teapot on a wooden table, yellow flowers, sunlit window,
an additional ceramic vase and lemons. No blank or corrupt decode. All 50
progress lines were received. Selected result, history and saved metadata
preserve model, prompt, negative, seed, shape, steps, CFG and scheduler.

Final PNG: `/home/alex/dif-inference/output/run/job-0005.png`.
SHA256 after metadata embedding:
`d917e8f31745bda1c8db3765c12281ee12fe14b93b3c997b91faf17f742a55a1`.
Exact native command: `output/run/difc/job-0005/argv.json`.
Worker/native receipt: `output/run/job-0005.png.difc_daemon_result.json`.

UI completion timer: **25.8 s**; worker wall 25.0235 s; native internal total
24.3275 s. Before-click automation timestamp `15:36:05.555Z`, final PNG mtime
`15:36:30.734449454Z` (25.179 s between those observations). PTF includes loading
and saved output; the worker/native timers exclude surrounding UI boundaries.
Fresh compiler process, warmed checkpoint/module caches after the smoke,
no resident model reuse. CPU test/build activity overlapped part of the run;
therefore this is a functional timing observation, not a controlled benchmark
or a speedup claim. Final generation cgroup peak/events were not captured before
the scope disappeared, so no pressure-free claim is made.

## Verification and remaining limits

Both release builds passed. Supported Rust workspace test run: 249 passed,
two existing graph tests ignored; the previously documented LTX context-cache
test explicitly filtered because its fixture lacks the creator checkout.
This is not a claim that the unfiltered workspace suite passes. Earlier stale
Mojo-worker/LoRA expectation failures are preserved in the lab.
Worker tests cover SDXL routing, negative prompt forwarding, progress parsing,
checkpoint mismatch, missing tokenizer, NaN CFG, unsupported editing/schedule
and attempts to override request arguments. Server tests cover native worker
selection and narrowed capabilities/defaults.

Lab: `/tmp/dif-sdxl-ui-20260906.JwH1fh` (builds, tests and server logs).
No commits/pushes. Existing dirty work in both checkouts preserved.

## Deferred Dev

Existing compact transformer:
`/home/alex/mojodiffusion/models/flux2dev/squareq_w4_r32` (15 shards, roughly
17 GiB; plan/index present). Current cache search included symlink-following
config/index inspection, named HF repositories and local text-encoder folders.
The former Dev snapshot and a usable Mistral encoder/tokenizer were not found.
Recorded open dependency: the current W4 tool also opens the removed original
transformer before replacing its weights; a future standalone-slab loader
change is needed. That change was not made after the user deferred Dev.
No synthetic/cached-prompt conditioning substituted for a real encoder.

## Follow-up: deployment configuration

The subsequent JSON deployment refactor moves the SDXL alias, defaults and
artifact manifest into the shared deployment document. Readiness now checks
the configured complete SDXL checkpoint, tokenizer and native executable,
not the inherited Mojo split-CLIP/VAE layout. The Euler/normal restriction is
also checked through the deployed preflight API. This is a control-plane
follow-up, not a new PNG quality or PTF measurement. See
`JSON_DEPLOYMENT_CONFIG_2026-09-06.md` for the follow-up test record.
