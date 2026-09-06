# P4 — Krea 2 Turbo / Raw served through the Diffusion Compiler (RTX 5080 16 GB)

Blocker (see P4_KREA2_BLOCKED_2026-09-05.md): the sampler read its initial latent
and schedule from creator fixtures, and the sealed bundles were gone.

## Compiler-side change (branch `dcui/krea2-native-initial-seed` in
/home/alex/diffusion-compiler, commit 5f5de8e; main untouched, nothing pushed;
`build-5080-release/difkrea2sample` and `difkrea2vae` rebuilt from it)

- `difkrea2sample --initial-seed N`: native initial latent reproducing
  `torch.randn((1,16,128,128), generator=torch.manual_seed(N), device="cpu",
  float32) -> bf16 -> 2x2 patch packing` (the ComfyUI / PyTorch-CPU convention).
  The creator fixtures drew CUDA Philox noise, which is not reproduced, so a seed
  here is not interchangeable with a creator seed. **Gate: bit-exact vs the
  PyTorch oracle** (`scripts/krea2_noise_oracle.py`, seed 20260831):
  262144 elements, max_abs 0, cosine 1 (`evidence/p4/noise-gate-summary.log`).
- `--reference` optional in `difkrea2sample` (skips the schedule byte-compare and
  trajectory metrics; report says `parity_reference: none`) and in `difkrea2vae`
  (finiteness-only gate). Fixture-driven runs are unchanged.

## Chain (`scripts/krea2_chain.sh`, raw checkpoints, no sealed DiT/VAE bundles)

diftokenize (541+5 tokens) → difcondition run --krea2 (Qwen3-VL-4B, program
+ bundle sealed once at /mnt/disk1/diffusion-compiler-cache/krea2-dcui) →
difkrea2text (TextFusion) → difkrea2sample --initial-seed (28-block MMDiT,
11000 MiB resident, rest streamed) → difkrea2vae (Qwen-Image VAE, 1024x1024).
Raw variant runs CFG with a negative conditioning pass.

| job | surface | variant | steps / cfg | seed | wall | viewed |
|---|---|---|---|---|---|---|
| run-1 | chain CLI | Turbo | 8 / 0 | 7 | 36 s | copper kettle: on-prompt |
| job-0025 | POST /v1/generate | Turbo | 8 / 0 | 99 | 32 s | (lighthouse desk) done |
| job-0023 | Generate button, prompt with `(film grain:1.1)` | Turbo | 8 / 0 | 31 | — | figs on marble: on-prompt, weight tag passed through |
| job-0024 | POST /v1/generate, negative "blurry, low quality, text" | Raw | 52 / 3.5 | 5 | 568 s | red fox at sunset: on-prompt |

Turbo: ~1.7 s per evaluation; Raw: ~5.3 s per step (two evaluations, streamed).
Comparator on record (3090 Ti, sealed BF16 bundles, creator fixture): 59.14 s
creator chain vs 2.2x native; this box has no creator timing yet.

Bugs found and fixed on the way: krea2 family still mapped to the Mojo worker
binary (jobs 19–21 "worker binary unavailable"); GPU-lock deadlock when the
chain script re-took the lock the worker already held (1 h timeout) — chain
scripts now skip the inner lock under `DIFC_LOCK_HELD`; first job after a worker
rebuild hits the stale-pipe retry (job-0022), as documented in memory.
