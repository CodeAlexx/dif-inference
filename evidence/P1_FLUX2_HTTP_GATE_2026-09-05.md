# P1 gate — FLUX.2 [klein] Base 9B through the HTTP product surface

GPU NVIDIA GeForce RTX 5080 16 GB, driver 13020, cuDNN 9.23. Compiler
build `/home/alex/diffusion-compiler/build-5080-release` (revision 07c7514 per
difprobe), used as-is. Server: forked serenity-server at this repo's commit,
worker `serenity_worker_difc`, port 7811. Checkpoint
`/home/alex/models/FLUX.2-klein-base-9B/flux-2-klein-base-9b.safetensors`,
creator VAE `flux2-intake/flux2-dev-creator-vae/ae.safetensors`, accepted
5080 W8A8 ConvRot + INT8 weight-only flags (config/difc.json). Comparator:
the accepted 2026-09-02 native replay (52.809 s median, 50 steps).

| job | prompt (mine) | steps | seed | boundary | result |
|---|---|---|---|---|---|
| job-0001 | red bicycle / stone wall | 50 | 7 | POST /v1/generate → poll /v1/job → PNG | done, 56.1 s wall incl. weight prep, progress 1..50 streamed, visual_health pass, **viewed: on-prompt** |
| job-0002 | lighthouse | 50 | 11 | cancel at step 10 | server said cancelled at 24 s but the tool SURVIVED ~50 s (mem-safe wrapper's systemd service orphaned by pgid SIGTERM) — **bug, fixed** |
| job-0003 | lighthouse | 8 | 11 | after the orphan | failed loud: compiler pressure gate `required=11.3 GB free_before=4.3 GB` (correct fail-closed) |
| job-0004 | lighthouse | 50 | 11 | cancel at step 10, fixed worker | cancelled at step 11 within 2 s, 0 surviving processes, VRAM 232 MiB |
| job-0005 | lighthouse | 8 | 11 | after cancel | done, 26.1 s wall (≈20 s prep + 8 steps) |

Fix: the worker applies the host cap itself with `systemd-run --user --scope`
(same MemoryMax/MemorySwapMax/MemoryHigh as scripts/mem_safe_runtime.sh) plus
the wrapper's MemAvailable admission rule, so the tool stays in the worker's
process group and cancel reaches it. Logs: p1-flux2-*.log in this directory;
per-job argv/stdout/stderr under output/run/difc/job-NNNN/.
