# Application profiling

`tools/profile.py` consumes a verified [same-build artifact](/systems/cpu-v3-tang-nano-20k/docs/application-build-artifact.md).
The application is never recompiled to discover an address or generate a report.

```powershell
python -B systems/cpu-v3-tang-nano-20k/tools/profile.py run target/application --backend system --frame 2 --budget 8000000 --seconds 600 --events 10000000 --out target/profile
python -B systems/cpu-v3-tang-nano-20k/tools/profile.py report target/profile --out target/profile-report
python -B systems/cpu-v3-tang-nano-20k/tools/profile.py compare target/before target/after --difference "application change" --out target/comparison.json
```

Use a fresh output directory. `plan` accepts the same arguments as `run`, freezes
the program/runtime and writes the request without executing the model. A plan
is not a measurement. Omit `--runtime` to build the normal model binary; this
separate build has a 600-second host limit and has no application producer.
`--runtime FILE` uses an explicitly frozen executable. Execution always has a
cycle/step budget and a host deadline; exhausted or truncated captures remain
available as partial evidence and return failure. Increase `--events` when the
event journal, rather than the machine budget, is the limiting resource.
Ordinary scenes reserve a separate `--report-seconds` budget (default 1,800);
capture plus reporting must fit the existing 7,200-second host ceiling.

| Backend | Machine and metric boundary |
| --- | --- |
| `system` | Complete CPU/cache/arbiter/MC/GPU/Display cycle model at 54 MHz. Explicit synthetic `preloaded-post-boot` entry; excludes SPI and real loader-state equivalence. Frame N ends at applied present N and requires its complete registered source raster. F1 includes initialization; later frames use applied-to-applied windows. GPU work is the sum of full owned submission lifetimes, separately from the window. |
| `cpu` | Native-edge CPU/cache with functional GPU. Actual successful host VBlank applications snapshot CPU advances, retired words, logic and native cycles; adjacent snapshots define the selected frame. GPU contention/execution time is absent. |
| `sim` | Instruction CPU with functional GPU. The same presentation boundary provides CPU steps/retired words; it has no cycle or millisecond interpretation. |

CPU/sim runs use 16,667-us host ticks and at most 10,000 samples; the explicit
execution budget also determines work per host interval. Comparisons require
that schedule to match. Host wall time is never a machine performance metric.
The [cold GPU replay](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md)
remains a separate captured-command boundary, not an application frame backend.

## Scene ownership

The default scene has disconnected input and selects the requested presentation
(default 2). Bloxorz's moving level-29 shadow scene owns its binding and input
state machine under `applications/bloxorz/tools`:

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bloxorz --start-level 29 --start-phase play --out target/bloxorz29
python -B systems/cpu-v3-tang-nano-20k/tools/profile.py run target/bloxorz29 --scene bloxorz29-shadow --backend system --budget 18000000 --seconds 1800 --events 32000000 --out target/shadow
```

Omit `--frame` for this scene: the application selects stable-A, commits a real
right-button read, acknowledges/releases input, then selects rolling-B. The
same-build manifest must declare initial level 29. Verified debug globals,
source statement locations and unique linked direct calls provide segment,
Execute and return PCs. Listing words and direct-call offsets must match the
actual executable. Missing/ambiguous bindings or an incompatible wait ABI fail.
Return endpoints use the final Execute word of the following instruction,
including any immediate prefix. `--start-phase play` initializes the application
in its ordinary resting state, excluding entry animation from warmup; the default
`entry` behavior retains the game animation. The model never patches game state.
No microsecond interval is invented when the executable has no timer calls.
The original v1 plan/codec remains available as an explicit application tool.
Gate arrivals are retained until the instruction retires, including a return
LOAD that waits for memory after Execute. No input is released before its real
read and acknowledgement. The ordinary recorder accepts its existing larger
event tier (up to 112 million events and a 4-GiB journal byte bound); the generic
System tier remains capped at 10 million events.

## Sealed runs and comparison

`run.json` is published last. It binds the request, package, executable, tool
identities, outputs, completion and machine metrics with SHA-256. Raw captures,
stdout/stderr and failures remain in the run directory. A run copied elsewhere
can be validated and reported without Cargo or a device. Re-rendering uses the
recorded presentation ordinal, including scene-selected frames.

Comparison rejects partial runs, changed evidence, different backends, clocks,
input, scenes, windows, schedules, executables, tools or selected application
state. Different program/resources require an explicit `--difference` description;
this declaration does not relax the other checks. Outputs give before/after,
absolute delta and percentage in the original metric units.

When the capture completed but offline processing failed or timed out, recover
it into a new run rather than executing the model again:

```powershell
python -B systems/cpu-v3-tang-nano-20k/tools/profile.py recover target/profile --out target/recovered --seconds 1800
```

Recovery first verifies the sealed source run, requires a reached, untruncated
System capture and all required scene rasters, then copies its frozen program,
runtime and raw evidence. It invokes only the offline reporter for the selected
frame. The original wrapper exit and tools remain provenance; the new analysis
has its own exit status, tool identity and source run ID. Only strict completed
analysis publishes a complete sample. A genuinely partial capture is rejected.
The source run and failed report are preserved. Reports can be large because
the warmup journal remains available for validation; host processing time is
excluded from all machine metrics.
