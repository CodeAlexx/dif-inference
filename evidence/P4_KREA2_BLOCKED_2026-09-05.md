# P4 — Krea 2 Turbo: BLOCKED (recorded, not hidden)

Findings, from the compiler source and the disk on 2026-09-05:

1. **Prepared bundles are absent.** The Krea 2 recipe
   (`perf/recipes/krea2-turbo-bf16-1024.json`) binds
   `artifacts/krea2-conditioner-2026-08-31/*.difbind` and
   `artifacts/krea2-denoiser-2026-08-31/*.difbind`; `diffusion-compiler/artifacts/`
   does not exist (gitignored) and no `krea2*.difbind` exists anywhere under
   /home/alex, /mnt/disk1, /mnt/disk2. Raw weights are present in the HF cache
   (`models--krea--Krea-2-Turbo`, `models--krea--Krea-2-Raw`) and could be re-sealed
   with `difweights make-krea2-{bf16,text-bf16,vae-bf16}-bundle`.
2. **`difkrea2sample` is a creator-fixture harness, not an arbitrary-prompt
   sampler.** `tools/difkrea2sample.cpp:224-236` makes `--initial-fixture` and
   `--reference` mandatory; `:510-548` reads the initial image tokens from the
   fixture (`initial_image_tokens`) and byte-compares the native schedule against
   the fixture's `timesteps`. There is no native seeded initial-noise path for
   Krea 2 (H3 has `difh3noise`; FLUX.2 generates noise in-process).
3. **Geometry is fixed.** `difkrea2vae` writes 1024x1024 only (`tools/difkrea2vae.cpp:542`).

Unblocking requires a compiler-side change (a `--seed`-driven native initial
latent in `difkrea2sample`, parity-gated against the creator RNG), then
re-sealing the bundles. Both are outside this directory's scope ("executables
used as-is"). The UI keeps the krea2 family with its full capability entry and
refuses generation at preflight with the reason (verified: p2-grid.log, first run).
