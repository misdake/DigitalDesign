# Ordinary level-29 profiling

The application owns this scenario, its host runner, and the interpretation of
its sidecar. The shared System does not know game phases, input actions or the
ordinary wait register convention. The scenario does not patch game RAM or
advance time artificially. Initial level and entry/play phase are explicit
application build inputs; the default level-1 entry keeps its animation.

## Entry and exact inputs

The [System profiling front door](/systems/cpu-v3-tang-nano-20k/docs/application-profiling.md)
builds a plan from one verified artifact via `tools/plan.py`. It resolves physical
globals, unique direct calls and source statements from that artifact, validates
listing words against code and checks the wait argument ABI. Return bindings use
the following instruction's final Execute word, including immediate prefixes.
`--start-level 29 --start-phase play` provides a resting start for bounded warmup;
selection still requires real neutral presentations and committed right input.
Missing/ambiguous bindings fail. Absent timer functions produce no invented sites.

Build `cargo build --release -p cpu-v3-tang-nano-20k --bin bloxorz-profile-runner`.
Launch a verified plan with:

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/bloxorz/tools/profile.py path/to/verified-plan.json
```

The previous `cpu-v3-profile-runner` and common `ordinary_runner.py` commands
have been replaced by these application-owned entries. Existing executable
identities are not interchangeable: bind a new plan to the actual new binary.

The plan remains `digitaldesign.ordinary-profile-plan`, version 1. `runtime`,
`program` and nonempty source/resource `bindings` contain file, byte length and
SHA256. The launcher verifies them before execution and creates a no-retry
`.launch.json` receipt. `output` and `report_output` must name new directories.
The runner requires `site_observation: "first-unheld-execute"`.

`words` maps names to physical CPU halfword addresses below 4194304, at most
64 probes: phase, moving, level (zero-based), x, z, orientation, mask,
transition, motion_age, epoch and switch_age_1 through switch_age_9 are required.
`gates` binds distinct before_input, consumed and sample segment/Execute-PC pairs.
`sites` binds vblank_return and final_wait_return, which may share a PC.
`clock_sites.clear/read` records code_segment and pc for actual timer MMIO.
All mappings describe the exact linked program. The direct v1 launcher consumes
them; automatic generation belongs to `plan.py`, never another compilation.

## Selection and timing

The first settled PLAY state has phase=0, moving=0, level=28, x=7, z=3,
orientation=0, mask=25, transition=0 and all nine switch ages at least 260 ms.
It warms one presentation and selects the next distinct presentation as stable-A.
Only after A's complete registered raster does a subsequent before_input gate
publish digital right (`0xffdf`). A committed Device5 channel-4 read of `0xdf`
and a committed acknowledge on channel 8..15 permit one neutral release at
the consumed gate. Watch probes do not count as reads.

Rolling-B selects the first sampled ROLL state (phase=1) with x=8, z=3,
orientation=1, mask=25, transition=0 and actual motion_age in [100,220) ms,
after read, acknowledge and release. An unexpected ROLL target is an error.
The selected output includes F1, F2, stable-A and rolling-B; their real startup
phases and presentation ordinals remain intact. Completion requires every
selected presentation's stable 800x480 registered raster.

Action gates retain an unheld Execute arrival until its instruction retires;
a cache-miss return LOAD need not retire on its Execute edge. Timing sites use
the first unheld Execute arrival. The current package has a dedicated final GPU
wait in `finish_frame`. The planner verifies the call and return instructions
and binds the actual channel-11 clear/read opcodes from the same-build listing.
This path needs no shared-wait register discriminator or resume epoch. Legacy
v1 plans retain their original shared-wait and epoch checks.

The codec checks committed physical Device0/channel-11 timer clear/read pairs,
read-before-edge prescaling and source PCs. Legacy plans also check the epoch.
Cold and saturated reads produce invalid intervals. It reports MMIO
intervals separately from vblank-return to final-wait-return intervals and from
applied-to-applied frame windows. The 900000-cycle comparisons require a valid
interval and bound source endpoints. Common interval activity accounting remains
in the system profiler.

## Bounds and report compatibility

Explicit ceilings are 80M application logic cycles, 112M main events / 4 GiB,
100K sidecar events, 7200 host seconds, four native presentation groups and
12 registered images. Plans supply smaller exact limits. Cold preparation is
bounded by 1M logic cycles and the same wall deadline. The model runs on one
8 MiB host worker; its simulated clocks and application stack are unchanged.
`max_total_host_seconds` includes identity checks, capture and report subprocesses,
is at most 7200, and reserves at least 60 seconds beyond `max_host_seconds`.
`max_native_command_images` is required and at most 2048; exceeding this export
budget preserves the raw capture and reports an error.

The launcher exports only completed selected rasters and records missing scopes.
Budget exhaustion retains an explicitly partial capture, not a successful game
measurement. The profiler's application-sidecar dispatch binds runner version 1
to this codec. Existing `runner-plan.json`, `runner-events.jsonl`, derived report
fields, hashes, timing semantics and scoped exports remain readable without
rewriting saved evidence. The legacy format name alone does not make it a
general-purpose scenario protocol.
The System front door reserves a separate report budget and can recover a
complete sealed capture into a new offline analysis run. It does not retry the
model or change the original failure; partial captures remain invalid samples.

## Qualification ownership

Selection tests live beside `tools/selection.rs` and run with
`cargo test --release -p cpu-v3-tang-nano-20k --bin bloxorz-profile-runner`.
Run the application codec/launcher tests with:

```powershell
python -B -m unittest discover -s systems/cpu-v3-tang-nano-20k/applications/bloxorz/tests -p 'test_*.py'
```

The short real-System input/timer/cache/observation parity contract remains
`cargo test --release -p cpu-v3-tang-nano-20k --test system_runner`.
Common profiler tests cover interval activity and formal sidecar report/image
binding. No synthetic trace or short model fixture claims a new full-game
performance or hardware result.
