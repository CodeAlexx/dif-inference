# P2 — SerenityUI (Mojo desktop app) → diffusion-compiler-ui parity checklist

Method: headless Chrome (google-chrome --headless=new, driven over the
DevTools protocol by scripts/ui_drive.mjs) against the live server on
127.0.0.1:7811 with the compiler worker. Every PASS row names the script or
log that produced it; screenshots are in evidence/ui/. "N/A (reason)" means
the served family (FLUX.2 [klein] Base 9B) cannot honor the knob and the UI
shows it greyed with the capability reason instead of posting it — the same
rule SerenityUI's own dead knobs (Precision/Attention/Offload) followed.
Families that are not yet ported keep their full knob sets and are refused at
preflight with the reason (p2-grid.log first run: krea2-turbo refusal).

## genparams v1 keys (the 28-key wire surface)

| key | result | evidence |
|---|---|---|
| model | PASS — picker lists 28 models across every SerenityUI family + H3 | /v1/models, p2-10-model-picker.png |
| prompt / prompt_raw | PASS — `<random:>` resolved per seed, raw kept | p2-syntax.log (job-0007) |
| negative | N/A for klein (no negative conditioning in the creator chain); field present, capability-hidden | p2-parity-structure.json `prompt.negative` |
| width / height | PASS — sliders + 8 aspect presets + swap | p2-parity-structure.json `resolution` |
| steps / cfg | PASS — sliders, family defaults hydrated (50 / 4.0 for Base 9B) | p2-01-landing.png |
| seed (−1 = random) | PASS — random toggle, shuffle, previous | `core` |
| sampler / scheduler | PASS — capability-filtered (Euler; Simple/FLUX.2) | `sampling` |
| variation_seed / variation_strength | PASS (present) / N/A for klein — section capability-hidden | `sampling.variation_*` |
| images 1..8 | PASS — queued as separate jobs (batch of 3 in p2-queue-cancel.log) | jobs 13–15 |
| init_image + creativity | PASS (present: file input, drop zone, preview, clear) / creativity N/A for klein | `source_image` |
| mask_image + lanpaint_mask_channel | PASS — Canvas tab mask paint / brush / eraser / lasso (LanPaint route) | `canvas_tab` |
| clip_skip / eta / sigma_min / sigma_max / restart_sampling | N/A — rendered as disabled rows with the runtime reason, never posted | `advanced_sampling` |
| vae | N/A — "Automatic / checkpoint VAE" disabled row | `refine_hires` |
| hires_scale / hires_denoise | N/A — server marks hires two-pass unsupported; Refine/Upscale rows disabled with reason | `refine_hires` |
| lora rows (0..2, weight 0..2) | PASS (section, picker, max-count note) — worker refuses LoRA for klein loudly | `lora`, worker `unsupported_knobs` |

## Sections and rails

| SerenityUI feature | result | evidence |
|---|---|---|
| Model section (task/model/vae/precision) | PASS — model + refresh; precision/task were dead knobs in SerenityUI | `model_section` |
| Resolution presets 1:1, 3:2, 16:9, 9:16, 512, swap | PASS — 512², 1024², 1152×896, 896×1152, 1344×768, 768×1344, 1280×832, 832×1280, swap | `resolution` |
| Sampling | PASS | `sampling` |
| Seed randomize | PASS | `core` |
| LoRA rows | PASS | `lora` |
| Images count | PASS | `core.images` |
| Init image validate/clear/thumbnail | PASS | `source_image` |
| Inpaint (mask) | PASS (Canvas tab) | `canvas_tab` |
| Presets save/load/page | PASS — Presets sub-tab; API round-trip 200 / listed / deleted | `presets` |
| Advanced knobs | N/A (see above) | |
| Grid (XYZ sweep) | PASS — NEW section; 2×2 seed×cfg grid composited in 86 s (grid-0012) | p2-grid.log, p2-07-grid-result.png |
| Queue rail: state, progress, per-row cancel | PASS — Remove on a queued row cancels server-side; Cancel current interrupts | p2-queue-cancel.log |
| History rail: newest first, star, reuse params | PASS — star persists via /v1/gallery favorites across reload; Reuse restored seed 2026 / steps 8 / prompt | p2-star.log, p2-history.log |
| Batch thumbnail strip | PASS — Current Batch panel | p2-04-after.png |
| Prompt syntax `(w:1.3)` pass-through | PASS | prompt-syntax.test.js (17/17), p2-syntax.log |
| Prompt syntax `<lora:name:w>` extract/merge/clamp | PASS (unit) — worker refuses LoRA for klein at submit | prompt-syntax.test.js |
| Prompt syntax `<random:a|b|c>` seeded pick, nesting | PASS | prompt-syntax.test.js, job-0007 |
| Nodes canvas (pan/zoom/select/wires/groups) | PASS — Workflows tab graph editor | `workflows_tab` |
| 87 node types | PARTIAL — 46 node classes in /object_info (the server's allowlist); SerenityUI's 87 included Comfy/Swarm/rgthree/kj/LanPaint aliases | `workflows_tab.node_types` |
| Comfy workflow JSON import / save / submit | PASS — Load Workflow (Ctrl+O), Save Workflow, Queue Prompt | `workflows_tab.buttons` |
| Status: health dot, backend label | PASS — "Idle" dot; runtime label "flux2 · Diffusion Compiler · admitted" | `status_bar` |
| GPU perf footer | NOT PRESENT — the web UI has no VRAM/util/temp footer (Settings → Memory exists) | p2-11-settings.png |
| Video tab | SerenityUI had none; this UI has Video sections + H3 Studio (H3 served by the compiler, P3) | |

## Verdict

All SerenityUI inference features are reachable except the GPU perf footer
(absent) and the node-type count (46 vs 87 aliases). Every unsupported knob is
visible with its reason rather than silently dropped.
