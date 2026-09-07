# Klein Base 4B / 9B compiler integration, RTX 3090 Ti

Date: 2026-09-06. UI clone origin `CodeAlexx/dif-inference`, initial commit
`4b102d2`; compiler branch `exp/h3-multifidelity-20260903`, initial commit
`7dad5e29f5a03f13decc62714f41cfe0c83fb46f`. Existing dirty compiler work retained.
No commits/pushes performed. Working evidence directory:
`/tmp/dif-klein-intake-20260906.zIrIko`.

## Implemented contract

Both variants use the same Rust worker and `difflux2sample` native C++/DiffIR
pipeline, with model configuration rather than a new runtime or GPU kernel.
4B: 3072 hidden, 24 heads, 5+20 blocks, 149 transformer bindings, Qwen3-4B
hidden 2560 / MLP 9728, taps 9/18/27, context 7680. 9B retains the existing
4096 hidden, 32 heads, 8+24 blocks and Qwen3-8B context 12288 path.
These local weights are **Base**, with 50-step CFG-4 defaults, not four-step
distilled variants. 4B low-precision flags are rejected pending their own gates.

Paths are explicit in `config/difc.json`; the worker refuses checkpoint
substitution, conflicting variant names and semantic overrides in extra_args.
Negative prompts, LoRA and editing are not advertised as supported for Klein.
The shared local VAE uses Diffusers names; the compiler aliases these to the
creator decoder, reshaping only corresponding 2D attention weights to 1x1
convolutions without changing their values.

Fastloader is off by default; exact `DIF_FASTLOAD=1` remains opt-in. The earlier
bounded staging/shared-weight lifetime and rotary fixes were not rolled back.

## Creator numeric gate: Base 4B

Creator source: `black-forest-labs/flux2` commit
`50fe5162777813d869182b139e83b10743caef15`, lab `creator/`.
Oracle command lives in lab `run-native-4b.sh`; implementation is compiler
`tools/gate_flux2_klein_intake.py`. Full real layers and 1024x1024 geometry,
two Euler steps, CFG 4, seed 4242. Prompt:
"A small wooden rowboat on a misty lake at sunrise, soft pastel sky."
Same local BF16 Qwen checkpoint on both sides; this does not claim equality
with the creator's default FP8 checkpoint. Initial noise, text and image IDs,
schedule and transformer inputs are held identical for the denoiser gate.

Bars declared before measurement: conditioning cosine >=0.999 and relative
L2 <=0.01; final latent cosine >=0.995 and relative L2 <=0.1; same-latent VAE
PSNR >=50 dB; trajectory PNG PSNR >=30 dB; zero nonfinite values.

The first implementation failed positive conditioning at 1.406% relative L2.
Source inspection found observable BF16 storage boundaries between RMS
normalization and learned scale multiplication, and between RoPE products and
addition. Expressing those boundaries with existing IR operations, enabled
only by the 4B factory, passed the original bars. No global kernel or accepted
9B/H3/Krea conditioning semantics were changed. The intermediate norm-only
attempt still failed at 1.269%; its evidence remains in the lab.

Final `4b-eager-oracle/report.json`:

| Check | Measurement |
| --- | --- |
| Positive conditioning cosine / relative L2 | 0.9999763355 / 0.0068796585 |
| Empty conditioning cosine / relative L2 | 0.9999991671 / 0.0012906639 |
| Final latent cosine / relative L2 | 0.9989909668 / 0.0450491504 |
| Same-latent decoder PSNR | 86.3125 dB |
| Creator-trajectory PNG PSNR | 47.7111 dB |
| Nonfinite / overall | 0 / PASS |

Metrics use Float64 accumulation. The initial oracle used Float32 CPU metric
reductions and produced an invalid cosine above one; that measurement bug was
fixed, and those initial cosine values are not acceptance evidence.
This is a two-step numerical gate, not a 50-step creator trajectory comparison.

## Browser and saved-output gates

Actual Chrome Generate-tab selection and button press; server at
`http://127.0.0.1:7811`. Base 4B selected 50 steps/CFG 4 automatically.
`job-0001`: 1024x1024, BF16, 50 steps, CFG 4, seed 4242, exact prompt above.
Finished PNG `output/run/job-0001.png` was viewed both in the UI and directly:
a coherent wooden boat, reflection, misty lake, distant trees and pastel dawn.
History/selected-result metadata matched model, seed, shape, steps and CFG.

Generate-click observation: `2026-09-06T15:16:02.012Z`; final PNG mtime
`2026-09-06T15:17:13.155557978Z`: approximately **71.14 s PTF to saved PNG**.
UI completion timer: 71.4 s; worker stage wall: 70.203 s. The latter is not
full PTF. Fresh compiler process, warm 4B disk/module caches after numeric
gates, no resident model reused and no competing compute job. One observation,
not a speedup claim, cold-start claim, or matched comparison with 9B/5080.

Base 9B: bounded two-step browser `job-0002` completed first, followed by
full 50-step `job-0003` with the same prompt/seed/shape/CFG. Its PNG was viewed
in the UI and directly: a coherent wooden boat and reflection beneath mist,
trees and a pastel dawn. History and selected-result metadata matched.
UI completion timer: **162.8 s**. The tool timestamp immediately before the
click action (`15:19:26.763Z`) to final PNG mtime (`15:22:10.409889491Z`) gives
a **163.65 s upper observation**, including browser-action delivery overhead;
it is not an exact click-event timestamp. Worker wall: 162.461 s; native
prompt_to_png: 161.935 s. Do not substitute either for the user-facing boundary.

9B used a fresh compiler process after its smoke, with a warm module cache
but checkpoint cache residency not guaranteed. The host cgroup reached exactly
24 GiB and recorded `max=241`, `high=0`, `oom=0`, `oom_kill=0` when sampled
after the final denoiser step. Thus this is a successful functionality/quality
gate **under reclaim pressure**, not an admitted performance baseline.
The 20,000 MiB planner held 199 tensors (17,720,954,880 weight bytes), streamed
436,207,616 bytes per step, required estimate 20,937,036,288 bytes.

Saved UI PNG SHA256, after metadata embedding:

- 4B `job-0001.png`: `80b60fdbe93d53ea3d381ebd4d953447a404d1023fafa8806d7c933dcda7b562`
- 9B `job-0003.png`: `1407fc8dfab568ab5fcd4a737a7dde91d4ed2dd2d824ca695ed59722958a8429`

The compiler's PNG hashes differ because they precede UI metadata embedding.
Native 4B hash: `65aaf52cbd0985145d2984cab880ee7e8fa5b327596564717f6a9ae310d8bb6e`;
native 9B: `0294f6ec49aeac1677a5e1cbd4697f3219e2c2a30380677928a2e53bf0dd16e7`.

## Tests and limits

Release Rust workspace builds. Current supported test run:
`rust-tests-supported-2.log`: 247 passed, two pre-existing graph tests ignored,
one LTX test explicitly filtered. The unfiltered run failed the unrelated
`video::tests::ltx2_context_cache_reuses_existing_prompt_entry` because its
synthetic fixture lacks the now-required creator checkout. This is not an
all-green full-suite claim; the failure receipt is retained in `rust-tests.log`.
The native CPU stub now comes from this checkout's worker, not a Mojo binary.

Final full compiler rebuild succeeded (113 build steps). Nine focused CTests
passed in 5.75 s: fastload admission, ASM I/O, resident prefetch, typed runtime,
tensor I/O, FLUX.2 frontend, rotary, tokenizer and FLUX.2 prompt. The build/test
cgroup had zero high/max/OOM events and peak 1,388,412,928 bytes. This is a
focused regression run, not a new full CTest-suite run. The frontend regression
explicitly checks 4B's eager rounding and unchanged default 9B semantics.

Shell syntax, model registration idempotence, JSON configuration and both
repository diff-whitespace checks passed. `ldd difflux2sample` shows native
CUDA/cuDNN/cuBLAS/C++ libraries, no Python/Torch runtime (cuDNN's shared library
is installed beneath the local Python package directory). Durable copies of
the numeric report, Rust/CTests logs, both PNGs and worker/native receipts:
`/home/alex/diffusion-compiler-docs/evidence/klein-base-4b-9b-20260906/`.

UI remains running on port 7811, idle with both results in history. Unrelated
port 7812 service was not touched. No model weights downloaded, no GPU jobs
left running, no commits or pushes. Existing compiler worktrees unchanged.

Existing Krea/H3 configuration paths are not revalidated on this host.
Dev's native compiler/Mistral path exists but is not integrated into this UI.
