# H3 authored geometry, and why four healthy renders were cancelled

2026-09-07, 3090 Ti box, `dif-inference` at d0a4477.

## 1. Six gates pinned H3 to the fixture shape

H3 Studio could only queue the sealed profile's geometry. A 768x768 shot was
refused four separate times, each by a different gate, and two more were found
by reading rather than by hitting them. In order of discovery:

| # | Gate | Refusal |
|---|---|---|
| 1 | `scripts/h3_compiler_runner.sh` geometry FATAL | (lifted earlier) |
| 2 | `minimax_h3.rs` `compiler_h3_geometry_document` advertised `sealed_native_profile` | UI hid every other shape |
| 3 | `h3-studio.js` client check keyed on that policy | client-side refusal |
| 4 | `difc-config` `h3_contract` | `H3 width=768 does not match configured sealed profile (832)` |
| 5 | `minimax_h3.rs` `validate_compiler_h3_request_in` | `H3 output duration must match the configured frame count; trimming/resampling is not implemented by this runner yet` |
| 6 | `h3_compiler_runner.sh` delivery check | `FATAL output must match configured delivery frames/FPS` |

Gate 5 compared `geometry.output_frames` (delivery frames, 120 for a 5 s shot)
against `profile.frames` (the 17-aligned internal count, 124). Those are
different quantities by construction: `internal_frames = align(model_output_frames
+ motion_context_frames)`. The stated reason -- no trimming -- does not hold:
the runner is invoked with `--output-frames=model_output_frames` and the encoder
with `--output-frames` plus `--trim-start-frames`, so both the head trim and the
tail cut are already plumbed. The check now verifies that the delivered frames
plus any continuation overlap fit inside the frames the pass generates.

ControlNet already had a correct generalized form of gates 4 and 6 and had been
rendering off-profile for days (video-0044, video-0051, both 1344x768x120
through this same chain). The other tasks were simply never given the same
treatment.

FPS stays sealed: delivery framing and the 17-frame alignment derive from it,
and no resampling stage exists.

### The gate that was not on the server

The Resolution control in the Shot inspector was a `<select>` of preset
options. Even with every server gate lifted, no resolution outside that list
was reachable from the UI. It is now a typed Width/Height pair validated
against the runtime's advertised `native_range`, with the presets retained as
a quick fill. Verified in the browser: typing 1152x640 is accepted and the
preset control reports `Custom - 1152x640`.

A related defect: `setShotField` cleared `prompt_override` for any field
outside an allowlist that included seed and steps but not width or height, so
changing resolution silently discarded an authored prompt. Geometry is a render
setting, not prompt content, and is now on that allowlist.

## 2. The runtime guard was cancelling on I/O burstiness

Four H3 renders (video-0051, 0061, 0062, 0063) were cancelled by
`scripts/runtime_guard.py` with nothing actually short of memory.

`Guard.observe` has two pressure rules. The first compares the kernel's own
`avg10` against `full_pressure_percent` / `some_pressure_percent`. The second
divides a stall-counter delta by a `pressure_window_seconds` window -- 2 s --
and compared the result against those same thresholds. A 2-second window and a
10-second average are not the same measurement, so the windowed rule was about
five times more sensitive than its numbers suggested.

Measured from the guard's own telemetry for video-0063
(`output/runtime-guard/serenity-runtime-memory-20260907-021450-*.jsonl`,
466 samples over 117.0 s):

| source | avg10 max | worst 2 s window |
|---|---|---|
| host | 1.15% | 14.93% |
| `user@1000.service` | 2.40% | 14.88% |
| child cgroup | 0.00% | 14.69% |

The child cgroup reported an `avg10` of exactly 0.00% while the windowed rule
computed 14.69% for it. Over the whole run the child peaked at 10.39 GB against
a 44 GB cap and `MemAvailable` never fell below 55.1 GB against a 16 GB desktop
reserve. Nothing was scarce; the signal was ordinary streamed-checkpoint I/O.

The windowed rule now has its own threshold, `window_pressure_percent` = 30,
chosen with roughly 2x margin over the measured healthy burst. The `avg10`
rules are unchanged, so sustained pressure still cancels: the recorded incident
(`avg10` 55.63%) was re-run against the new policy and still cancels with
`parent full memory pressure avg10 reached 10%`. systemd-oomd's own
`ManagedOOMMemoryPressureLimit` is a sustained `some_pressure_percent`, which
the `avg10` rule reaches first.

`scripts/test_runtime_guard.py` gained a regression for the burst case. The
existing `test_recent_stalls_cancel_before_ten_second_average_catches_up`
fixture used 250000 counts over 2 s = 12.5%, which is *below* what a healthy
render produces, so it was raised to 700000 (35%). The property it tests -- a
burst cancels before `avg10` catches up -- is unchanged.

## 3. Page cache is not the cause

An earlier theory was that the streamed loader fills the page cache and the
guard reacts to reclaim. Recorded here so it is not re-derived:

- `MappedStorage::discard` issues `MADV_DONTNEED` on the mapping;
  `MappedStorage::evict` additionally issues `POSIX_FADV_DONTNEED` on the file.
  Streamed constants deliberately use `discard`, keeping the page cache warm so
  the next evaluation does not reread from disk (`src/runtime/tensor_io.cpp`).
- The streamed working set is small: `STREAMED_PREFETCH_PLAN tensors=233
  bytes_per_iteration=1666522688` (1.67 GB) with `warm_page_cache=1`. The
  runtime's own `host_cache_working_set_fits` check had correctly decided
  warming was affordable.

1.67 GB per iteration cannot account for a 52 GB page cache, and the guard
telemetry shows the cancels happened with 55 GB available. Page-cache pressure
was not the mechanism.

## 4. mem_safe_runtime RUNTIME_MAX

24G -> 44G. cgroup-v2 charges page cache to the reading cgroup, so a job
reading an 18.16 GB checkpoint alongside device staging exceeded the cap and
the kernel reclaimed the pages being read. Measured on this box, identical
binary and flags, warm cache:

| | total | resident upload |
|---|---|---|
| unwrapped | 75.0 s | 20.6 s |
| wrapped, `MemoryMax=24G` | 163.9 s | 109.5 s |

62 GB host minus the 16 GB desktop reserve leaves 46 GB, so 44 GB keeps the
reserve intact. The dynamic admission gates still refuse against live
`MemAvailable`, so this is a ceiling, not a grant.

## Verification

- `difc-config`: 14/14. `json_aliases_and_h3_profile_drive_admission` was
  updated -- it asserted the sealed-geometry behaviour, and its `ref2va`
  assertion had been passing only incidentally, because `config/h3-ref2va.json`
  carries no geometry and the sealed comparison failed on a null rather than on
  task gating.
- `scripts/test_runtime_guard.py`: 14/14 (3 live-only tests skipped).
- Three `serenity-server` tests fail (`ltx2_context_cache_reuses_existing_prompt_entry`,
  `minimax_h3_compiler_references_preserve_audio_order_roles_and_staging`,
  `readiness_shape`). They predate this work and reproduce with these changes
  stashed.

## 5. The modulation cache was discarded for the wrong reason

After the gates were lifted, an authored resolution still ran ~4x slower than
the fixture shape. The runner dropped the sealed AdaLN modulation cache whenever
width, height or frame count departed from the profile, and compact AdaLN plus
CUTLASS scaled-all depend on that cache, so every off-profile render also lost
the INT8 fast path.

The cache carries its own description:

    __meta__.kind             = adaln-modulation
    __meta__.row_layout       = per-evaluation-sorted-unique-padded-v1
    __meta__.steps            = 20
    __meta__.distinct_timesteps = 38
    __meta__.nblocks          = 50

and holds 50 tensors of shape (114, 32256), one per block, where 114 = 3 x 38
distinct timesteps. Nothing in it is sized by width, height or frames.

The remaining question was whether the timestep schedule itself moves with
resolution -- if it did, the cached rows would be keyed to the wrong timesteps.
It does not. Comparing the per-evaluation `video_t` values logged by the same
shot rendered at 768x768 with the cache discarded (video-0067) and retained
(video-0068), all 19 evaluations agree exactly:

    diff <(grep -o 'video_t=[0-9.e-]*' video-0067/runner.log) \
         <(grep -o 'video_t=[0-9.e-]*' video-0068/runner.log)   ->  0 differing lines

So the cache is valid at any geometry, and only a different schedule or block
count invalidates it. The clearing condition is now `STEPS` and `BLOCKS` only.

### Measured

Same shot, same seed 1000, same convrot-int8 route, same 768x768:

| run | modulation cache | mean denoiser |
|---|---|---|
| video-0067 | discarded | 47,121 ms/step |
| video-0068 | HIT | 10,936 ms/step |

4.31x. Both completed with `guard_rc=0 child_rc=0`.

This is **not** bit-parity and should not be described as such: PSNR between the
two is 24.67 dB average / 22.97 dB luma. The frames are the same scene, subject,
pose, framing and lighting -- the divergence is the INT8 compact-AdaLN/CUTLASS
route versus online modulation, and H3's sample is known to be chaotic to small
perturbations in the early evaluations. The cached route is the one the sealed
profile has always used; the slow online path was what off-profile geometry had
been forced onto.
