# Shared deployment JSON: scope and verification

Date: 2026-09-06. Requested: move machine/model paths and configurable runtime
settings out of hardcoded deployment logic. No model downloads, kernel changes,
precision changes, new generation, commits or pushes in this refactor.

## Implementation

- Added shared native Rust `difc-config`, used by server, compiler worker,
  registration and shell runners. It resolves inherited JSON once per process;
  shell settings are flattened once, not parsed by a new process per setting.
- `config/runtime.json` contains shared build/launch settings, model aliases,
  defaults, registration, artifact descriptors, and H3 profile/staging/decode
  policy. Machine files extend it with paths and overrides. `${repo}`, `${home}`,
  `${config}` and dotted-key references are literal substitutions, not `eval`.
- H3 readiness checks the actual configured runner, native tools, source shard
  indices/files, sealed programs/bundles, ConvRot pack and exact modulation
  cache. The direct checked-in runner replaces the missing installed script.
  Configured geometry/task/precision/attention/cache choices gate admission.
- Image artifact reports use the same configured full-checkpoint/native-worker
  route as generation. SDXL no longer reports missing inherited Mojo CLIP/VAE
  split files. Canonical checkpoint mismatch protection is retained.
- Registration is idempotent and refuses conflicting files/symlinks or path
  traversal. The memory wrapper preserves explicit profile selection across
  its clean service environment.

## Verification record

Lab: `/tmp/dif-json-config-20260906.jgnIfT`.

- Nine config tests cover literal paths/references, inheritance/merge, cycles,
  malformed fields, registry conflicts/traversal, changed aliases/profile
  admission, and malformed H3 prerequisites.
- Supported workspace test command (the existing LTX fixture-dependent test
  is explicitly excluded, not silently counted as passing):

  ```bash
  env MEM_MAX=24G MEM_HIGH=infinity SWAP_MAX=2G DESKTOP_RESERVE=16G \
    scripts/mem_safe_runtime.sh cargo test --workspace \
    --manifest-path serenity-server/Cargo.toml -j4 -- \
    --skip video::tests::ltx2_context_cache_reuses_existing_prompt_entry
  ```

- The artifact regression exposed an empty-suffix path-join bug: a checkpoint
  was reported with a trailing slash, producing `Not a directory`. Both the
  failed regression and failed live readiness responses are preserved. The
  join now leaves complete file paths unchanged when no suffix is configured.
- Alternate inherited JSON located outside the repository, with spaces in its
  filename and compiler path, resolved correctly. The actual memory-wrapper
  boundary preserved `DIFC_CONFIG` and returned the literal overridden path.
- Repeated registration was idempotent. Shell syntax checks passed.

Final verification (`tests-artifact-path.log`, `build-artifact-path.log`):
260 tests passed; two existing graph tests ignored; one fixture-dependent LTX
test filtered. Release build passed. Test/build cgroup peaks were respectively
1,004,900,352 and 1,051,967,488 bytes; high/max/OOM events were all zero.

After restarting the idle task-owned server on port 7811 with that build:

- SDXL Base 1.0 and Klein Base 4B/9B preflight each reported `admitted: true`,
  `artifact_profile.ready: true`, native production entries and no missing files.
- SDXL Karras was rejected with the explicit Euler/normal-only error.
- H3 reported `available: false`; native `check-h3 int8` exited 1 with exactly
  seven missing prerequisites. No generation was submitted.
- All five existing image jobs remained `done`. Health reported backend `difc`.
  Other servers and GPU owners were not interrupted.

Final responses are `final-preflight-*.json`, `final-video-readiness.json`,
`final-h3-check.json`; server log is `server-final.log`. Earlier failed evidence
was preserved. Final shell syntax and `git diff --check` passed.

## Limits and remaining work

No H3 video was generated here. Local H3 source weights and the ConvRot pack
exist, but six configured sealed programs/bundles and the exact T2VA modulation
cache are absent. Different keyframe modulation caches are not substitutes.
The recorded 5080 video success remains historical evidence, not a new host
quality or performance gate.

This config pass covers compiler-backed deployment routes. Legacy unserved
backends still contain inherited constants; model math, operation names,
request-schema identifiers and minimal bootstrap layout remain code. The
descriptor checks do not prove full checkpoint integrity, numerical parity or
output quality. Config changes require a server restart. No inference speedup
is claimed; fastloader remains opt-in, and PTF retains the complete
prompt/Generate-to-saved-PNG/MP4 timing boundary.
