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
cargo run -p cpu-v3-tang-nano-20k --bin cpu-v3-boot-assets
```

The CPU V3 system build script generates Stage0, Stage1, the demo application, and boot image data
from `systems/cpu-v3-tang-nano-20k/rcc` into Cargo `OUT_DIR`. Use `cpu-v3-boot-assets` to materialize
those exact files for packing or programming. Never check in a second hand-maintained instruction
or Flash byte array.

## Validation

Use the nightly toolchain selected by `rust-toolchain.toml`. On Windows, initialize an MSVC x64
developer environment and ensure Cargo precedes Git Bash tools on `PATH`.

```text
cargo test --workspace
cargo clippy --workspace --all-targets
powershell -ExecutionPolicy Bypass -File scripts/check-layering.ps1
powershell -ExecutionPolicy Bypass -File scripts/check-source-hygiene.ps1
```

Use `scripts/validate-hardware.ps1 -Mode quick|iverilog|audit|pnr|all` for repeatable hardware
validation. `audit` validates existing artifacts without rebuilding; `pnr` builds and audits them.
Neither mode invokes the programmer.

Use `hardware/vendor/gowin/scripts/run_board_validation.ps1` for board work. Its default
`Audit` mode is offline; `Observe` never programs; `Program` and `Full` perform each requested
hardware write once and never reset USB or retry programming automatically. UART validation
recognizes both DDHT status and structured CPU V3 `CV3B` boot-error frames.

External Verilog tests use `IVERILOG_EXE` and `VVP_EXE` when set, otherwise
they resolve `iverilog` and `vvp` through `PATH`.

CPU V3 emulator-vs-RTL co-simulations are `#[ignore]` tests in `ip/cpu-v3` that drive the same
stimulus through the Rust model and the RTL in Icarus and compare cycle by cycle. They cover the
three modules with standalone cycle-accurate peers: the four-entry fetch queue, the two-way cache,
and the `CpuV3Core` itself. The core one is mandatory for every step that touches the CPU pipeline,
forwarding, retirement, or the data/handshake paths (including Stage 12 fetch/execute overlap and the
GPR forwarding mux):

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label "cpu-v3 emu/rtl co-sim" -CargoArgs @("-p", "cpu-v3", "--lib", "--", "--ignored", "--nocapture", "--test-threads=1")
```

The core co-sim is `ip/cpu-v3/src/hardware/mod.rs::tests::core_emu_matches_rtl_pipeline_overlap`.
Its program is `core_cosim_program()` in the same file (a dependent `ADDI` chain that must run one
instruction per cycle, a wide SETP load, a taken branch, an async store whose `data_write_data` must
equal the forwarded `r0`, an FPU barrier, and a halt). It compares `pc`, segments, `retired_words`,
halt/fault, `halt_signal`, and the instruction/data handshake each cycle, so a change that breaks the
overlap, forwarding, or halt value is caught directly. The fetch (`fetch.rs`) and cache (`cache.rs`)
co-sims sit alongside it.

The system-level co-simulation `systems/cpu-v3-tang-nano-20k/tests/system_cosim.rs` drives the
composed RTL (core, fetch queue, I-cache, D-cache, memory arbiter, behavioral SDRAM) in Icarus
against the shared cycle-accurate system model (`tests/system_emu/mod.rs`), comparing the core
ports cycle by cycle and the post-flush SDRAM contents exactly. It is mandatory for changes
touching the core, fetch, cache, arbiter, or memory paths:

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label "cpu-v3 system co-sim" -CargoArgs @("-p", "cpu-v3-tang-nano-20k", "--test", "system_cosim", "--", "--ignored", "--nocapture", "--test-threads=1")
```

## Cargo output summarization

Run cargo through `scripts/run-cargo.ps1` instead of invoking `cargo` directly when the raw
output would be large. It tees the full log to `target/cargo-summaries/<label>.log`, prints a
compact summary (exit code, warning/error counts, per-`test` pass/fail totals and failure list,
or a bounded tail for `run`), and writes `target/cargo-summaries/<label>.json`.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label "workspace tests" -CargoArgs @("--workspace")
& scripts/run-cargo.ps1 -Subcommand clippy -Label "strict clippy" -CargoArgs @("--workspace", "--all-targets", "--", "-D", "warnings")
& scripts/run-cargo.ps1 -Subcommand run -Label "board build" -CargoArgs @("-p", "digital-design-hardware-gowin", "--example", "board_health", "--", "--build")
```

`-CargoArgs` is a `string[]`; pass it in-process with `&` and splatting. Do not use
`ValueFromRemainingArguments` or `powershell -File`, both of which swallow `-p`/`-D`-style flags.
`validate-hardware.ps1` and `run_board_validation.ps1` already route every cargo call through it;
extend the same pattern for any new cargo step.

Every simulator test must supply a maximum cycle/step count. Keep project files, comments, and
documentation in English. Do not commit unless the user explicitly asks for a commit.
