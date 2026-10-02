# GPU v2 IP

Reusable GPU components live here. The `gpu-v2` crate depends on the generic
[`audited`](../../modeling/audited/README.md) modeling tool and has no CPU or
board-system dependency. The earlier standalone cmodel remains under the Tang
Nano system as a historical development harness; new components belong here.

See [architecture](docs/architecture.md) for component ownership and the model
directions, [lighting](docs/lighting.md) and [frontend](docs/frontend.md) for the
implemented Rust models.
The [triangle setup oracle](docs/triangle.md) is complete only through its first
Rust step; counted/timed and hardware directions remain future work.
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
