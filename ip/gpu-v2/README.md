# GPU v2 IP

Reusable GPU components live here. The `gpu-v2` crate depends on the generic
[`audited`](../../modeling/audited/README.md) modeling tool and has no CPU or
board-system dependency. The earlier standalone cmodel remains under the Tang
Nano system as a historical development harness; new components belong here.

See [architecture](docs/architecture.md) for component ownership and the model
directions, [lighting](docs/lighting.md) for the numerical models, independent
cycle emulator, synthesizable RTL and matched isolated PnR evidence, and
[frontend](docs/frontend.md) for the bounded Rust frontend models.
Lighting provides explicit Fast/Compact resource profiles with scalar H
normalization, fixed DSP roles and checked synchronous retained storage.
Additional system candidates compare scalar N/H, direct squares and a complete
ID FIFO at II2/II4; their area and numerical limitations are recorded together.
The [interactive Rust review](web/README.md) compares the real Rust kernels with
shared scene inputs and optional error maps through a local WebSocket server.
The [functional oracle chain](docs/oracle-chain.md) renders actual meshes through
fetch, vertex, setup/coverage, lighting/sampling, final, ROP and atomic caches.
Its two configurable native Rust pipelines compare frontend precision and normal
transport without executing counted/timed code or a JavaScript shader.
Optional Rayon acceleration preserves the same ordered numerical results. A
[lighting workbench](web/README.md#lighting-scheduling-workbench) displays and
edits the real bound DAG's repeated calendar, with independent resource checks.
The [triangle setup oracle](docs/triangle.md) is complete only through its first
Rust step; counted/timed and hardware directions remain future work.
The [texture models](docs/texture.md) implement oracle/counting and universal
periodic preparation connected to bounded cache/color and cycle-MC execution.
An independent register-only sampler now executes the complete numerical,
demand-cache/refill and Color path, with bounded whole-sampler RTL differential
tests and two lossless preparation optimizations. Its conservative serial
calendar complements the periodic Runtime; fitted area remains open. The
[Sampling viewer](web/sampling-schedule.html) shows certified leaf calendars
and actual preparation/cache/controller traces from the project models.
The [framebuffer model](docs/framebuffer.md) provides a materialized depth/blend
oracle and bounded serial/overlapped control through the common burst port.
The older control fixtures retain their assumed arithmetic latency; a separate
audited ROP leaf, actual registered cache emulator and synthesizable cache/leaf
RTL now have independent numerical and complete-image differential tests.
The [controlled pixel composition](docs/pixel-system.md) connects global16
basic/light/sample stores, ordered join and exact final to the existing double
output/ROP and actual serial cycle MC. The original J1 uses external branch
stimulus. New compact quad/status/common-context dispatch connects actual
Lighting/Sampling, registered Final/ROP/cache and a single shared MC clock owner,
with CPU/display traffic and whole-image guards. Unlit/untextured bypass their
branch queues. The integrated dispatcher/geometry path is still Rust-only;
isolated component RTL is not an integrated GPU RTL or board result.
The [SDRAM memory controller combination](docs/sdram-memory-controller.md) is
provided by the Gowin vendor crate through a dev dependency. Its Rust oracle
supports real data, calibrated average service and configured CPU/display loads.
The independent [scheduler](../../modeling/scheduler/README.md) supports bounded
batch planning. Target specifications and ongoing experiments remain in the local
GPU v2 design documents until their contracts have been implemented and verified.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label gpu-v2 -CargoArgs @('-p','gpu-v2')
& scripts/run-cargo.ps1 -Subcommand clippy -Label gpu-v2-lint -CargoArgs @('-p','gpu-v2','--all-targets','--','-D','warnings')
```
