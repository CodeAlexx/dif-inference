# H3 enhancement placement: source audit

Read-only research for Diffusion Compiler, not an optimization of SerenityFlow
or MojoDiffusion. Inspected local algorithmic source on 2026-09-06; no features
ported, enabled or performance/quality-admitted by this report. Canonical
compiler checkout was on `exp/h3-multifidelity-20260903`, HEAD `7dad5e2`, with
existing dirty work; source paths below refer to that inspected working state.

## Put behavior in its owning layer

| Behavior | Compiler-stack owner |
| --- | --- |
| Packing, conditioning, ControlNet, model-specific LoRA layout, motion-context semantics | H3 frontend lowering to shared operations |
| Fusion, precision choice, approximate-cache policy, residency plan | Compiler transformations / execution plans |
| Staging, pinned storage, prefetch, streams/events, lifetimes, vendor calls | Shared native runtime |
| GPU-specific attention/GEMM implementation | Backend kernels under generic operations |
| Machine paths, feature selection and tested deployment defaults | Deployment JSON |
| Workflow, controls, progress, optional output enhancement | UI/control plane; no second inference executor |

This follows the compiler's [ownership tenet](/home/alex/diffusion-compiler-docs/TENETS.md:19).
Source frameworks are references for behavior, not mandatory scaffolding.

## Already present versus candidates

- **Fused BF16 RMSNorm + modulation already exists in the compiler**, with
  rounding-boundary preservation in its lowering:
  [compiler.cpp](/home/alex/diffusion-compiler/src/compiler/compiler.cpp:2113).
  Mojo's related implementation is
  [minimax_h3_dit.mojo](/home/alex/mojodiffusion/serenitymojo/models/dit/minimax_h3_dit.mojo:1067).
- **ConvRot/CUTLASS and owned/CK attention routes already exist** in
  [cuda_executor.cpp](/home/alex/diffusion-compiler/src/runtime/cuda_executor.cpp:8795).
  Presence is not proof that the UI enables a route, that every GPU supports
  it, or that it improves complete PTF. The configured UI H3 runner currently
  chooses exact cuDNN attention, not every available experimental backend.
- **Streaming/prefetch mechanisms already exist** in the compiler runtime:
  [cuda_executor.cpp](/home/alex/diffusion-compiler/src/runtime/cuda_executor.cpp:12368).
  SerenityFlow's source shows useful explicit upload-event and lifetime handling:
  [loader.py](/home/alex/serenityflow/serenityflow/models/minimax_h3/loader.py:578).
- **Mojo's middle-block residual reuse is a distinct candidate**, not identical
  to the compiler's current EasyCache. Mojo recomputes front/back bands and
  uses separate video/audio probe checks, accumulated budgets and exact tail
  steps: [decision logic](/home/alex/mojodiffusion/serenitymojo/models/dit/minimax_h3_step_cache.mojo:553),
  [pipeline integration](/home/alex/mojodiffusion/serenitymojo/pipeline/minimax_h3_t2va.mojo:1969).
  The compiler's existing
  [EasyCache loop](/home/alex/diffusion-compiler/tools/difh3infer.cpp:1321)
  can reuse a whole evaluation's delta. These need separate quality contracts.
- **Exact text/audio query-prefix attention is another candidate**: Mojo
  overwrites the approximate prefix with exact cross-attention over full K/V:
  [minimax_h3_dit.mojo](/home/alex/mojodiffusion/serenitymojo/models/dit/minimax_h3_dit.mojo:432).
  It is a quality-protection mechanism, not evidence of a free speed increase.
- **Modulation caches are not interchangeable across stacks.** SerenityFlow
  validates a four-slot v2 cache identity, scheduler/timesteps and dependencies:
  [loader.py](/home/alex/serenityflow/serenityflow/models/minimax_h3/loader.py:411).
  The compiler validates a sorted-unique-padded v1 layout:
  [cuda_executor.cpp](/home/alex/diffusion-compiler/src/runtime/cuda_executor.cpp:3697).
  Do not substitute a cache based only on matching step/block counts.
- **ControlNet and motion context are frontend feature candidates**, not generic
  attention optimizations: [control.py](/home/alex/serenityflow/serenityflow/models/minimax_h3/control.py:87),
  [motion_context.mojo](/home/alex/mojodiffusion/serenitymojo/models/minimax_h3/motion_context.mojo:404).
  No equivalents were found in the inspected canonical compiler `src`,
  `include` and `tools` search; this is a search result, not an exhaustive proof.

## Do not infer acceleration from names

SerenityFlow's owned INT8 eligibility explicitly restricts device/layout/dtype:
[int8_runtime.py](/home/alex/serenityflow/serenityflow/models/minimax_h3/int8_runtime.py:107).
Its NVFP4 wrapper dequantizes a dense weight before `F.linear` in `forward`:
[nvfp4.py](/home/alex/serenityflow/serenityflow/models/minimax_h3/text/nvfp4.py:171).
Neither filename establishes universal GPU support or a native FP4 GEMM win.

Next engineering decision: inventory which existing compiler plans are admitted
for each GPU, then test a specific missing candidate with matched real inputs,
decoded video/audio and complete PTF. No aggregate speedup is claimed here.
