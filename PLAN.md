# diffusion-compiler-ui — inference serving layer for the Diffusion Compiler

Written 2026-09-05 after a read-only inventory of `/home/alex/diffusion-compiler`
(HEAD 74fc0f4), `/home/alex/diffusion-compiler-docs`, `/home/alex/mojodiffusion/
serenity-server` (Rust control plane + canvas web UI) and `/home/alex/SerenityUI`
(pure-Mojo desktop app). Nothing was built or modified during the inventory.

## Assignment (Alex, 2026-09-05)

> in a separate directory, make an inference frontend for it, mojo serenity ui
> parity, use rust server. The executables can already be compiled; no reason to
> recompile if not needed.

Reading, stated as assumptions:

1. **Separate directory** = this directory. `/home/alex/mojodiffusion` and
   `/home/alex/diffusion-compiler` are not modified.
2. **Rust server** = the serenity-server workspace (crates `wire`, `ipc`, `graph`,
   `server` + `canvas/` web UI), **forked** into `server/` here so it can be
   changed freely without touching mojodiffusion. Its worker contract
   (socketpair + newline JSON, `serenity_worker_<key>` binaries) is kept verbatim.
3. **SerenityUI parity** = everything a user can do in the Mojo desktop app
   (the `serenity.genparams.v1` 28-key surface, 11 left-panel sections, queue +
   history rails, presets, single-axis grid, `<lora:>`/`<random:>`/`(w:1.3)`
   prompt syntax, node canvas + Comfy workflow import) is reachable in the web
   UI served here, verified by a checklist with evidence, not asserted.
4. **Compiler binaries are used as-is** from
   `/home/alex/diffusion-compiler/build-5080-release` (Release, sm_120, the build
   every accepted 5080 measurement cites). No rebuild of the compiler.
5. In compiler vocabulary a "frontend" emits DiffIR. This directory is a
   **server / worker / product boundary**, and is named that way in code.

## What exists (measured)

| Piece | Fact | Consequence |
|---|---|---|
| Compiler inference | Process chains only (`diftokenize` → `difcondition` → sampler → decoder → mux). No library install, no HTTP, no cancel hook, no callback. Progress = stdout lines `FLUX2_NATIVE_STEP`, `KREA2_NATIVE_STEP`, `H3_STEP`. | Worker spawns processes, pipes stdout, parses step lines, SIGTERM/SIGKILL to cancel. |
| FLUX.2 [klein] Base 9B | `difflux2sample` is a single prompt→PNG process; accepted 5080 config recorded in `/mnt/disk1/diffusion-compiler-artifacts/flux2-klein-base-9b/phase4/*-report.json` (52.8 s median, 50 steps, W8A8 ConvRot + INT8 weight-only, resident 14000 MiB). Refuses to overwrite outputs. | First family. Fresh output dir per job. |
| MiniMax-H3 T2VA / FL2VA / Ref2VA | Complete fixture chain at `/mnt/disk1/diffusion-compiler-cache/h3-5080-basic`, recipes in `perf/recipes/h3-*-convrot-exact.json`. Conditioner + denoiser programs are compiled **per token count and geometry**; the fixtures are 439-token 832x480x124. `difh3infer --serve` exists but buffers stdout (no streaming progress) and cannot be cancelled. | Second family. Arbitrary prompts need a program-prep stage per request (`difcondition program/bundle`, `difc make-h3-denoiser`, `difweights make-h3-denoiser-bundle`, `difmodcache`). Cost must be measured before the design is fixed. |
| Krea 2 Turbo | Prepared bundles (`artifacts/krea2-*`) are absent from disk; `difkrea2sample` usage requires `--initial-fixture`/`--reference` creator fixtures. Raw weights are in the HF cache. | **BLOCKED** until bundles are regenerated and the fixture dependency is understood. Reported, not hidden. |
| serenity-server | 245 Rust tests, 29 lowering goldens, ComfyUI-compatible `/prompt` + `/ws`, `/v1/generate` + `/v1/progress` WS, `/v1/video`, GPU lease, model registry scan of `$SERENITY_MODEL_ROOT`. Worker dispatch table in `crates/server/src/capabilities.rs`. Path dep `genesis-web` at `vendor/genesis/web` (9.4 MB). Any lowered key without a `GenerateRequest` field is silently dropped. | Fork server + `vendor/genesis`. Route admitted families to one new worker. New keys must be added in `wire::JobParams`, `GenerateRequest`, and `params_from_generate_request`. |
| canvas web UI | 42 JS files, 8 tabs (Generate, Canvas, Workflows, Video Edit, H3 Studio, Models, Queue, Settings). Already a superset of SerenityUI's surface except client-side prompt syntax (to verify). | Parity is inherited; verify by checklist. |
| SerenityUI | Talks to the Rust server on 127.0.0.1:7801 via `/v1/health`, `/v1/models`, `/v1/generate`, `/v1/jobs`, `/v1/cancel/:id`, `/v1/grid`. No video controls, no settings panel, no trainer hooks. | The parity checklist is §Parity below. |
| Box | RTX 5080 16 GB, driver 13020, cuDNN 9.23, ffmpeg present, GPU idle. `scripts/mem_safe_runtime.sh` + `flock /tmp/dc-gpu.lock` are the standing run conventions. | Worker runs heavy stages under the same lock and memory cap. |

## Layout

```
/home/alex/diffusion-compiler-ui/
  PLAN.md                  this file; state notes appended per phase
  server/                  fork of mojodiffusion/serenity-server (crates + canvas), + vendor/genesis
    crates/difc-worker/    NEW: serenity_worker_difc — the compiler-backed worker
  config/difc.toml         paths: compiler build dir, cache dirs, model files, lock file, mem cap
  models/                  SERENITY_MODEL_ROOT for this server (symlinks into real weight locations)
  output/bin/              worker binaries the server spawns (built here)
  output/run/              server out_dir: jobs.db, job-NNNN.png, uploads/
  evidence/                per-phase gate outputs (immutable, dated)
  scripts/                 launch, gate, and parity-check scripts
```

## Worker design — `serenity_worker_difc` (Rust, `crates/difc-worker`)

- Implements the IPC contract exactly (`{"ev":"ready"}` on start, `start`/`cancel`/
  `sampling_ack` in, `progress`/`done`/`failed`/`cancelled` out, unknown `cmd`
  ignored).
- Family is chosen from `JobParams.model` via the fork's registry classification;
  each family maps to a **chain**: an ordered list of compiler processes with
  argv templates, run in a fresh per-job work directory under `out_dir`.
- Heavy stages run as `flock <lock> mem_safe_runtime.sh <argv>`; stdout is
  parsed line-by-line for the family's step regex → `progress` events with
  `phase:"sampling"` and `total` from the request's steps.
- `cancel` → SIGTERM the current child (SIGKILL after 5 s), remove partial
  outputs, emit `cancelled`.
- `done.output_path` = `<out_dir>/<job_id>.png|mp4`; the compiler's own
  `--report` JSON is copied to `<output>.difc_daemon_result.json` so
  `/v1/job/:id/result` carries the compiler's receipt (revision, target
  fingerprint, hashes, timings). `params_json` embedded as the PNG
  `serenity.genparams.v1` tEXt chunk.
- Knobs the compiler cannot honor (sampler ≠ euler, clip_skip, eta, sigma_*,
  restart_sampling, vae override, LoRA) are **refused loud** with a `failed`
  event naming the knob, per the wire crate's own rule; nothing is silently
  dropped. Admission tables in `capabilities.rs` are narrowed to what each
  chain actually supports so the UI never offers a knob the worker refuses.

## Phases and gates

Each gate = my chosen input, through the product surface (HTTP or browser
clicks), output verified, evidence saved under `evidence/<phase>/`.

- **P0 fork + build.** Copy server + vendor/genesis (no `target/`), fix the
  genesis path, `cargo build --release`, `cargo test --workspace` (expect
  245 green, goldens unchanged). Add the difc-worker crate with a `stub` family
  that emits a PNG so the existing `stub_seam` IPC test passes against it.
  Gate: tests green; `tests/e2e_stub.sh`-equivalent passes with the new worker.
- **P1 FLUX.2 klein via HTTP.** Family table → `serenity_worker_difc`;
  model root with the klein checkpoint; chain = accepted 5080 `difflux2sample`
  configuration. Gate: `POST /v1/generate` → WS progress 1..50 → PNG on disk,
  on-prompt on inspection; `POST /v1/cancel` mid-run → `cancelled` within 5 s,
  worker respawns and the next job succeeds; result JSON carries the compiler
  report; wall recorded (expect ~53 s + cold prep).
- **P2 browser parity.** Launch server, drive the canvas in Chrome through the
  Generate tab → image in history; then walk the §Parity checklist item by
  item, recording pass/fail/absent with a screenshot or response body each.
  Gate: the checklist table in `evidence/p2/` with no unverified rows.
- **P3 H3 video.** Measure per-request program prep cost first (conditioner
  program+bundle, denoiser program+bundle, modcache) for one new prompt; decide
  prebuilt matrix vs on-demand prep from the numbers. Re-point the server's
  `/v1/video` minimax_h3 arm at the compiler chain (t2va first; fl2va with
  uploaded keyframes second). Gate: prompt of my choosing → MP4 with audio,
  viewed; progress streams; cancel works.
- **P4 Krea 2.** Determine whether `difkrea2sample` can run without creator
  fixtures (read the source, not the usage line). If yes: regenerate bundles
  with `difweights make-krea2-*-bundle` from the HF-cache weights and add the
  chain. If no: record BLOCKED with the exact reason and stop.

## Parity checklist (SerenityUI → this UI)

genparams v1 keys: model, prompt, negative, width, height, steps, seed (−1 =
random), cfg, sampler, scheduler, variation_seed/strength, images 1..8,
init_image + creativity, mask_image + lanpaint_mask_channel, clip_skip, eta,
sigma_min/max, restart_sampling, vae, hires_scale/denoise, lora rows 0..2.
Sections: Model (task/model/vae/precision), Resolution presets (1:1, 3:2,
16:9, 9:16, 512, swap), Sampling, Seed (randomize), LoRA rows, Images, Init
image (validate/clear/thumbnail), Inpaint, Presets (save/load/page), Advanced,
Grid (axis seed/cfg/steps/sampler/scheduler + values). Rails: Queue (state,
progress, cancel per row), History (newest first, star, reuse params from PNG
tEXt), batch thumbnail strip. Prompt syntax: `(text:1.3)` pass-through,
`<lora:name:w>` extraction/merge, `<random:a|b|c>` seeded pick. Nodes: canvas
ops, 87 node types, Comfy workflow JSON import, save/load, submit graph.
Status: health dot, backend label, GPU perf footer. Each row is marked
`PASS` (evidence), `FAIL`, or `NOT-APPLICABLE` (e.g. a knob the compiler
refuses, shown greyed with the reason).

## Standing rules carried in

Measure, do not assume. Fix, do not ask. No CPU compute in hot paths (host is a
parking lot). No changes in mojodiffusion, ~/dev/eri, or the compiler repo.
Never lower a bar to pass. Every timing names GPU, geometry, dtype,
checkpoint, seed, boundary, comparator. Kill stale resident workers after
rebuilding a worker binary. Serialize GPU work behind the lock.

---

## State note — 2026-09-05 (autonomous session, Fable 5.1)

- **P0 DONE.** Fork at `serenity-server/` (+ `vendor/genesis`, `serenitymojo/configs`),
  workspace 189/189 tests green, IPC seam test green against the new worker.
- **P1 DONE.** `crates/difc-worker` → `output/bin/serenity_worker_difc`. FLUX.2 [klein]
  Base 9B through `POST /v1/generate`: 50 steps in 56 s wall (job-0001, viewed,
  on-prompt), cancel reaches the compiler process within 2 s (job-0004), worker
  applies the host cap with a systemd scope (the mem-safe *service* orphaned the tool
  under cancel — fixed). Evidence: `evidence/P1_FLUX2_HTTP_GATE_2026-09-05.md`.
- **Families.** Every SerenityUI family is registered (28 models in `models/`) and keeps
  its full capability entry; families without a compiler chain are `not_ported` and
  refused at preflight with the reason (`capabilities::compiler_serves`).
- **P2 DONE.** Headless-Chrome gates (`scripts/ui_drive.mjs`): Generate click → image
  (job-0006), prompt syntax port + 17-case Node gate (job-0007), new Grid (XYZ) section
  (grid-0012, 2×2 in 86 s), queued-job cancel (jobs 13–15), stars persisted via
  `/v1/gallery` favorites, reuse-params. Checklist:
  `evidence/P2_PARITY_CHECKLIST_2026-09-05.md`. Gaps: no GPU perf footer; 46 node
  classes vs SerenityUI's 87 aliases.
- **P3 DONE (T2VA, 832x480x124 @ 20 steps).** Per-prompt prep: fresh seal 7 min vs
  rebind 0.15 s (byte-identical). `scripts/h3_compiler_runner.sh` behind the server's
  H3 runner protocol. Gates: runner CLI run-1, `POST /v1/video` video-0017 (171 s),
  Generate-tab click video-0018 (178 s) — all viewed on-prompt with audio. Evidence:
  `evidence/P3_H3_VIDEO_GATE_2026-09-05.md`. Open: keyframe/ref/continue tasks, other
  geometries and step counts (fail loud).
- **P4 DONE (Alex: "fix krea").** Compiler-side `--initial-seed` (branch
  `dcui/krea2-native-initial-seed`, oracle-gated bit-exact) + optional `--reference`;
  chain from raw checkpoints; Turbo 32 s / Raw (CFG 3.5 + negative) 568 s through
  HTTP and the Generate button, all viewed on-prompt.
  `evidence/P4_KREA2_SERVED_2026-09-05.md`.
- **Also added:** GPU perf footer (`/v1/gpu`), H3 defaults to the compiler profile.

Launch: `scripts/serve.sh` (port 7811). Config: `config/difc.json`. Gates:
`scripts/gen.py`, `scripts/ui_drive.mjs` + `scripts/ui_*.js`.
