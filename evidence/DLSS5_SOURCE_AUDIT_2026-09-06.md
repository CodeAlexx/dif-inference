# DLSS 5 Visual Enhancer: agent source audit

Requested read-only audit of Merserk/dlss5-visual-enhancer. Pinned commit:
`8ac6ccce62c12c47e35ce98901fc75cb8d4a0e6b`. No application code executed,
packages installed, weights downloaded or GPU work submitted. The findings
below concern visible source behavior, not independent validation of demos.

## Actual pipeline and scope

The video path decodes existing video, prepares RGBA frames and temporal motion
guides, invokes a persistent external native DLSS session, then encodes/muxes
the result. It does not execute H3 denoising or replace its attention/GEMM.
[Source: processor.py](https://github.com/Merserk/dlss5-visual-enhancer/blob/8ac6ccce62c12c47e35ce98901fc75cb8d4a0e6b/src/neural_rendering/video/processor.py#L249-L457).

Motion guides use CPU DIS optical flow on reduced-size grayscale images,
scaled back to output geometry. A mean frame-difference threshold resets
history at scene changes. This addresses temporal guidance but does not prove
occlusion behavior, identity preservation or flicker-free output.
[Source: guides.py](https://github.com/Merserk/dlss5-visual-enhancer/blob/8ac6ccce62c12c47e35ce98901fc75cb8d4a0e6b/src/neural_rendering/video/guides.py#L18-L64).

Frame interpolation is a separate native DLSSG path, generating intermediate
frames with supported multipliers or cascaded passes. It is not equivalent to
the source model generating those frames.
[Source: scheduler.py](https://github.com/Merserk/dlss5-visual-enhancer/blob/8ac6ccce62c12c47e35ce98901fc75cb8d4a0e6b/src/frame_interpolation/scheduler.py#L29-L103).

## Reusable engineering lesson

The interpolation pipeline has bounded admission and four single-owner stages:
decode, guide preparation, native execution and encoding. It reserves memory
credits before producing results and drains downstream work first. Its queue
budget excludes history, scratch and codec buffers; it is not a whole-process
memory cap.
[Source: pipeline.py](https://github.com/Merserk/dlss5-visual-enhancer/blob/8ac6ccce62c12c47e35ce98901fc75cb8d4a0e6b/src/frame_interpolation/pipeline.py#L34-L218).

This scheduling pattern is portable research for the shared native runtime.
It is not a reason to add a second model executor or mandatory Python worker.
The architecture skill informed this placement recommendation: generic
mechanisms stay shared; vendor calls remain backend-specific.

## Integration boundaries and blockers

The visible runtime paths target Windows native executables/DLLs and NVIDIA
DLSS/NGX. This implementation is not a drop-in Linux, AMD-GPU or Intel-GPU
enhancer. Actual runtime capability detection matters, not just the GPU name.
[Source: paths.py](https://github.com/Merserk/dlss5-visual-enhancer/blob/8ac6ccce62c12c47e35ce98901fc75cb8d4a0e6b/src/core/paths.py#L7-L20).

The public tree intentionally omits release-only native payloads. The central
neural algorithm/weights cannot be source-ported from this repository. MIT
covers original application code, not all bundled third-party components;
native/add-on provenance and distribution permissions remain prerequisites.
[Binary/license boundary](https://github.com/Merserk/dlss5-visual-enhancer/blob/8ac6ccce62c12c47e35ce98901fc75cb8d4a0e6b/bin/README.md#L3-L19).

Recommended experiment location: optional enhancement after VAE decode and
before final encode, behind a vendor capability boundary, with selection and
runtime paths in JSON. Keep the original output available for comparison.

Unchanged H3 generation followed by enhancement adds work. Generating fewer
pixels or frames and reconstructing afterward might reduce complete PTF, but
changes the quality contract and is unmeasured. Test final resolution/frame
rate, motion, faces/hands/text, scene cuts, audio sync, total memory and
prompt-to-final-file time. Include runtime preparation: this project's video
timer starts after `prepare_runtime()`.
[Timing source](https://github.com/Merserk/dlss5-visual-enhancer/blob/8ac6ccce62c12c47e35ce98901fc75cb8d4a0e6b/src/neural_rendering/video/processor.py#L90-L98).
