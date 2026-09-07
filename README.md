# dif-inference

Rust server and browser UI backed by the native C++ Diffusion Compiler.
The generation worker invokes compiler executables; neural execution is native
C++, not a Python or Mojo model runtime. The H3 shell runner still uses Python
for tensor-header/math/result-file control utilities. Historical gates and the
original plan are in `PLAN.md` and `evidence/`.

## Local setup

The default `config/difc.json` is the September 6 RTX 3090 Ti profile. It uses
local BF16 Klein Base 4B / Base 9B checkpoints, their respective Qwen3 encoders,
and the shared F32 Diffusers-layout FLUX.2 VAE. SDXL Base 1.0 uses the local
full checkpoint (both CLIP towers and VAE included), F16 UNet/CLIP and BF16 VAE.
No weights are downloaded.
Edit the paths for another machine. The prior RTX 5080 profile is preserved
as `config/difc.5080.json`; its performance and precision policies are not
assumed to transfer to other GPUs.

```bash
bash scripts/build.sh
bash scripts/register-image-models.sh
bash scripts/serve.sh --port 7811
```

Open http://127.0.0.1:7811 and select Generate. Both **Base** models default
to 50 steps, CFG 4, Euler/simple. They are not the four-step distilled models.
The checkpoint registry uses symlinks; registration refuses to overwrite an
existing file or a symlink pointing elsewhere. Large weight files are ignored
by Git.

SDXL Base 1.0 defaults to 50 steps, CFG 7, Euler/normal. Its negative prompt
is supported; LoRA, editing, alternate samplers/schedules and VAE overrides
are not wired into the compiler worker. This is an explicit Base checkpoint
profile, not admission of every similarly named SDXL checkpoint.

Supported Klein controls: text prompt, seed, dimensions, steps and CFG.
Negative prompts, LoRA, image/reference input and editing are not currently
supported by this compiler worker and are rejected rather than ignored.
FLUX.2 Dev exists in the compiler CLI (`difflux2sample --flux2-model dev`),
but is deferred from this UI at the user's request: the compact SquareQ W4
transformer remains local, but a usable Mistral encoder/tokenizer was not found
in the current cache. No BF16 Dev download was made. Krea requires its configured
sealed artifacts and is outside the current H3 work.

H3's local sealed artifacts and native modulation cache are now configured.
The compiler runner supports T2VA plus first-frame (I2VA), last-frame (L2VA),
and first/last-frame (FL2VA) inputs at 832×480, 124 frames, 24 FPS, 20 schedule
points, cuDNN attention and Exact denoising. Use the H3 Studio opening/ending
frame fields; keyframe images must already be 832×480.
For Ref2VA, select **MiniMax-H3 References / Ref2VA** in Generate, open
**Source Image**, and use **Add files**. The ordered reference editor is shared
with H3 Studio: image thumbnails, reorder/remove, visual roles, notes, and audio
reference/reuse/voice-timbre controls. Limits come from `config/h3-ref2va.json`
(currently nine images, three audio files, twelve references total). Use
`<Picture N>` and `<Audio N>` in the prompt according to the displayed order.
Video-reference ingestion is not connected. Ref2VA INT8 uses the installed Mojo
W8A8 cache; it does not require converting those weights to ConvRot.
For H3 ControlNet, select **MiniMax-H3 Base / FL2VA** and open **Source Image →
Ordered ControlNet guides** (also in H3 Studio's **LoRA / Control** inspector).
Upload image/video guides, select Prepared or Canny and crop/pad/stretch, and set
each guide's strength and timestep range. Optional inpainting takes a paired
source and mask; white repaints, black preserves, with an invert switch.
This ports Mojo's media processing and 49-channel Union packing into the existing
native runner. Short inputs hold their final frame. Deployment paths, limits and
defaults are in `minimax_h3.control` in `config/difc.json`.
The current gates and remaining work are in
`evidence/H3_NATIVE_RUNNABILITY_2026-09-06.md`.

Fastloader is off by default in the companion compiler. Leave `DIF_FASTLOAD`
unset for the standard loader; only explicit `DIF_FASTLOAD=1` opts into the
experimental path. GPU jobs share `/tmp/dc-gpu.lock` and a 24 GiB host-memory
cap. The default profile does not reuse the 5080 INT8 recipe.

## Deployment JSON

`config/runtime.json` owns shared launcher/build settings, registered model
aliases, request defaults, artifact manifests and H3 runtime/decode policies.
`config/difc.json` extends it with this machine's paths and model settings;
`config/difc.5080.json` preserves the other machine's profile. The server,
worker, registration and shell runners use the same native `difc-config`
resolver. Set `DIFC_CONFIG` to select another JSON file and restart the server
after changing it (the running server keeps a startup snapshot).

```bash
DIFC_CONFIG=/path/to/machine.json bash scripts/build.sh
DIFC_CONFIG=/path/to/machine.json bash scripts/register-image-models.sh
DIFC_CONFIG=/path/to/machine.json bash scripts/serve.sh
serenity-server/target/release/difc-config resolve
serenity-server/target/release/difc-config get minimax_h3.runner
serenity-server/target/release/difc-config check-h3 int8
serenity-server/target/release/difc-config check-h3 int8 --task fl2va
bash scripts/prepare-h3.sh
```

Each file may `extends` another file, relative to its containing directory.
Objects merge recursively; arrays replace. Strings support literal `${repo}`,
`${config}`, `${home}` and `${dotted.config.key}` references, never shell
evaluation. `${config}` refers to the selected top-level file's directory.
Cycles, unknown references and invalid checked fields fail at startup.
For example, an alternate profile can extend the default file and override
only `server.port`, `compiler_build` or individual model paths.

The scope is the compiler-backed deployment routes, not removal of every
constant from the inherited application. Model math, native operation names,
request-schema identifiers and bootstrap repository layout remain code;
legacy unserved backends still contain old configuration constants. New model
aliases select implemented native families, not arbitrary new model math.
See `evidence/JSON_DEPLOYMENT_CONFIG_2026-09-06.md` for checks and limits.

## Validation

`evidence/KLEIN_4B_9B_INTAKE_2026-09-06.md` records this host's gates and
limitations. PTF means Generate/prompt to the finished saved PNG/MP4, including
loading. Compiler-stage timing alone is not UI PTF. State the process, cache,
residency and queue conditions when comparing runs.

SDXL's Generate-button gate and current test results are recorded in
`evidence/SDXL_UI_GATE_2026-09-06.md`.
