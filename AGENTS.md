# DigitalDesign project guide

Read `ARCHITECTURE.md` before moving code across crates. The dependency direction is
`circuit -> hardware -> ip/compiler -> systems`; arrows mean “may be depended on by”.

## Workspace map

- `circuit` (`digital-design-circuit`): circuit graph, simulation, and Verilog rendering.
- `hardware/core`: hardware description, projects, resources, and test framework.
- `hardware/common`: vendor-independent FPGA shell modules.
- `hardware/vendor/gowin`: Gowin toolchain, primitives, reports, and targets.
- `ip/common`: physical word addresses, memory protocol, layouts, and device channels.
- `ip/cpu-v1`, `ip/cpu-v2`, `ip/cpu-v3`: processor IP and target RCC backends.
- `compiler/rcc`: Rust-subset frontend, IR, optimization, and register allocation machinery.
- `compiler/tools`: the multi-target `rcc` command.
- `systems/cpu-v1-sim`, `systems/cpu-v2-sim`, `systems/cpu-v3-tang-nano-20k`: final systems,
  system-only tools, applications, and firmware.

CPU V2 `src/isa.rs` and `src/isa.html` define ISA v2.6 and must not be modified by structural work.
CPU V2 target code is in `src/rcc_backend`; CPU V3 has its own independent `rcc_backend`.
`compiler/rcc` must not import either CPU crate.

## RCC and boot commands

```text
cargo run -p compiler-tools --bin rcc -- input.rs --target cpu-v2
cargo run -p compiler-tools --bin rcc -- input.rs --target cpu-v3 --code-base 0x200
cargo run -p cpu-v2-sim --bin rcc-run -- input.bin 1000000
cargo run -p cpu-v2-sim --bin rcc-dbg -- input.bin
cargo run -p cpu-v3-tang-nano-20k --bin cpu-v3-pack -- manifest
```

The CPU V3 system build script generates Stage0, Stage1, the demo application, and boot image data
from `systems/cpu-v3-tang-nano-20k/rcc` into Cargo `OUT_DIR`. Never check in a second hand-maintained
instruction or Flash byte array.

## Validation

Use the nightly toolchain selected by `rust-toolchain.toml`. On Windows, initialize an MSVC x64
developer environment and ensure Cargo precedes Git Bash tools on `PATH`.

```text
cargo test --workspace
cargo clippy --workspace --all-targets
powershell -ExecutionPolicy Bypass -File scripts/check-layering.ps1
```

External Verilog tests use `IVERILOG_EXE` and `VVP_EXE` when set, otherwise
they resolve `iverilog` and `vvp` through `PATH`.

Every simulator test must supply a maximum cycle/step count. Keep project files, comments, and
documentation in English. Do not commit unless the user explicitly asks for a commit.
