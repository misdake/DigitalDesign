# Reusable GPU v2 IP

GPU components and standalone composition live in this crate. Board systems own
CPU/device submission, physical memory integration, clocks, firmware and Display.
The CPU system binds the production command GPU through its Device4 leaf.

The [specification index](/ip/gpu-v2/docs/README.md) follows a render from host submission
to memory-visible output. Start with the [overview](/ip/gpu-v2/docs/overview.md) and
[submission sequence](/ip/gpu-v2/docs/submission.md), then consult encodings, arithmetic
and execution details as needed. The selected standalone factory is
`system::command::verilog`. It directly assembles the command, frontend,
producer-pool, sampling, lighting and framebuffer owners. The checked
`system::vliw_sdram::verilog` interface remains a separate standalone composition.

[Independent reference APIs](/ip/gpu-v2/docs/references/README.md) have explicitly different
formats and execution scopes. `system::functional` and `system::vliw_cycle`
provide current command result-level and cycle execution respectively. CPU
resource handles and Display ownership belong to the system firmware.

Unimplemented top-level, module and software designs stay outside Git in the
local project design directory. After a part is implemented and verified, its
actual spec and implementation approach are merged into docs and the completed
design part is removed. Development progress belongs in project todo and daily/
weekly logs, not in this specification directory. Each directory maintains its
own README list; historical performance/resource ledgers are not updated.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label gpu-v2 -CargoArgs @('-p','gpu-v2')
& scripts/run-cargo.ps1 -Subcommand clippy -Label gpu-v2-lint -CargoArgs @('-p','gpu-v2','--all-targets','--','-D','warnings')
```

Bounded RTL replay usage is in the owning [integration example](/ip/gpu-v2/examples/vliw_integration/README.md).
Host grouping, corpus coverage and protocol replay are described in the
[test guide](/ip/gpu-v2/tests/README.md).
Validation of a component or standalone GPU is not full CPU/GPU/Display or board validation.
