# DigitalDesign

Full-stack digital design in Rust: a gate-level circuit simulator, a hardware
description framework that targets Gowin FPGAs, custom 16-bit CPU IP, a
Rust-subset compiler (`rcc`), and fitted FPGA systems — including a complete
CPU V3 machine running on the Tang Nano 20K board with two-stage flash boot,
SDRAM, and HDMI display.

Everything is developed and verified in one workspace. Design flow, CPU
microarchitecture, and compiler are version-controlled together, so the
hardware-under-test, its emulator, the compiler that programs it, and the
benchmark evidence live at the same commit.

## Highlights

- **Gate graph, simulation, and Verilog export** (`circuit`): wire/NAND-level
  circuit model with a deterministic cycle simulator.
- **Hardware description framework** (`hardware`): one Rust type keeps a
  module's emulation, NAND implementation, and Verilog contract together;
  modules derive their own Verilog identity and can mix emulated and structural
  implementations. Gowin targets carry typed board I/O, capacity accounting
  (LUTs, FF, SSRAM, BSRAM, DSP, PLL, fitted memory), and export into a complete
  Gowin project (wrapper, CST, SDC, project XML, build script).
- **Three CPU generations** (`ip`):
  - **CPU V1**: pilot processor with assembler, core, reference model, and an
    abstract device bus; runs Sokoban in `cpu-v1-sim`.
  - **CPU V2**: 16-bit Harvard CPU, ISA 2.6, cycle model, and rcc backend.
  - **CPU V3**: current ISA (revision 0.7), Stage 12 microarchitecture with
    overlapped integer execution, FPU (`fix16`, `vec2/3/4`), fetch queue,
    two-way I-cache, write-back D-cache with asynchronous store, and a
    hardware multiplier.
- **`rcc` compiler** (`compiler/rcc`): a tiny strict subset of Rust syntax —
  every valid rcc program is also valid Rust, so rust-analyzer works on it with
  no plugin. It compiles through target-independent IR, optimization, and
  register allocation to CPU V2 or CPU V3 binaries, with listings and debug
  info.
- **Fitted FPGA system** (`systems/cpu-v3-tang-nano-20k`): 22-bit physical
  memory map, 320x240 HDMI framebuffer, system-control/boot/display device
  channels, two-stage flash boot, and the boot package. Simulator tools and
  a single-page web debugger (`cpu-v3-dbg`) target the same sources the build
  script compiles into boot images.
- **Verification depth**: modules are tested against emulation, NAND, and
  explicit Icarus HDL simulation from one shared typed-vector test; CPU V3 has
  cycle-by-cycle emulator-vs-RTL co-simulation (core, fetch queue, cache, and
  full system) plus a frozen benchmark suite.

## Repository layout

```
circuit/                         gate graph, simulation, Verilog rendering
hardware/
  core/                          hardware description and project/resource APIs
  macros/                        hardware derive macros
  common/                        vendor-independent FPGA shell components
  vendor/gowin/                  Gowin tools, primitives, and board targets
ip/
  common/                        physical-memory and device-channel contracts
  cpu-v1/                        reusable CPU V1 processor IP
  cpu-v2/                        CPU V2 ISA, model, and rcc backend
  cpu-v3/                        CPU V3 ISA, model, Gowin-bound cache/RTL, and rcc backend
compiler/
  rcc/                           frontend, target-independent IR and passes
  isa-macros/                    ISA definition macros
  tools/                         multi-target rcc command
systems/
  cpu-v1-sim/                    CPU V1 memory, devices, display, and programs
  cpu-v2-sim/                    CPU V2 runner and debugger
  cpu-v3-tang-nano-20k/          fitted full FPGA system and boot chain
```

Dependencies point upward through the layers: `circuit -> hardware ->
ip/compiler -> systems`. A later layer may use an earlier layer, never the
reverse. See [`ARCHITECTURE.md`](ARCHITECTURE.md) for the detailed ownership
rules and the memory/device model.

## Prerequisites

- **Rust** — the nightly toolchain pinned in [`rust-toolchain.toml`](rust-toolchain.toml)
  is selected automatically. On Windows, initialize an MSVC x64 developer
  environment and ensure Cargo precedes Git Bash tools on `PATH`.
- **Icarus Verilog** (optional) — explicit HDL simulation and the emulator/RTL
  co-simulations use `iverilog`/`vvp`; set `IVERILOG_EXE`/`VVP_EXE` or let them
  resolve through `PATH`.
- **Gowin EDA** (optional) — synthesis, place-and-route, and programming the
  Tang Nano 20K, via `--gowin-home`, `GOWIN_HOME`, or `PATH`.

The checked-in `scripts/*.ps1` assume Windows/PowerShell development. The cargo
commands and rust toolchain are cross-platform; on other platforms, adapt or
reimplement the PowerShell validation scripts (the underlying cargo commands,
`IVERILOG_EXE`/`VVP_EXE`, and Gowin paths all work there unchanged).

## Build and validate

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets

powershell -ExecutionPolicy Bypass -File scripts/check-layering.ps1
powershell -ExecutionPolicy Bypass -File scripts/check-source-hygiene.ps1
powershell -ExecutionPolicy Bypass -File scripts/validate-hardware.ps1 -Mode quick
```

`validate-hardware.ps1` accepts `quick | iverilog | audit | pnr | all`. Run
cargo through `scripts/run-cargo.ps1` when raw output would be large — it tees
the full log and prints a compact summary.

## Tools

```text
cargo run -p compiler-tools --bin rcc -- input.rs --target cpu-v2
cargo run -p compiler-tools --bin rcc -- input.rs --target cpu-v3 --code-base 0x200
cargo run -p cpu-v2-sim --bin rcc-run -- input.bin 1000000
cargo run -p cpu-v2-sim --bin rcc-dbg -- input.bin
cargo run -p cpu-v3-tang-nano-20k --bin cpu-v3-dbg -- input.rs
cargo run -p cpu-v3-tang-nano-20k --bin cpu-v3-pack -- manifest
cargo run -p cpu-v3-tang-nano-20k --bin cpu-v3-boot-assets
```

`cpu-v3-dbg` compiles a CPU V3 rcc source file (or a directory whose `main.rs`
is the entry) in-process and serves a single-page web debugger. Library code
under `rcc_std/` is stepped over and hidden from the call stack.

```text
cargo run -p cpu-v3-tang-nano-20k --bin cpu-v3-dbg -- input.rs
usage: cpu-v3-dbg <input.rs | input-dir> [--code-base N] [--stack-init N] [--port 8322] [--no-open]
```

It opens the browser automatically on `http://127.0.0.1:8322` and provides
source tabs per module, breakpoints, step/over/out/continue, variables, call
stack, registers/F0-F15/ACC, and memory.

## Documentation

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — layer map, ownership rules, memory and
  device model, and board bring-up strategy.
- [`AGENTS.md`](AGENTS.md) — development guide, validation commands, and coding
  conventions.
- CPU V3 IP: `ip/cpu-v3/docs/` (ISA, microarchitecture, structure diagram).
- CPU V3 system: `systems/cpu-v3-tang-nano-20k/docs/` (architecture, boot image
  format, flash layout, optimization history).
- rcc language: `compiler/rcc/src/frontend/spec.md`.
- Hardware framework: `hardware/core/README.md` and
  `hardware/vendor/gowin/scripts/README.md`.

## License

Licensed under the [MIT License](LICENSE).