# CPU V3 IP documentation

These documents describe the reusable CPU V3 ISA and processor IP. Board-specific boot, SDRAM,
display, device allocation, and fitted timing belong to the Tang Nano 20K system documentation.

## Documents

- [RCC CSEG linking](rcc-cseg.md): explicit placement, far-call ABI, per-bank layout and products.

- [`testing.md`](testing.md): private unit-suite ownership, grouped core/backend
  cases and the boundary to system qualification.
- [`isa.md`](isa.md): normative ISA, architectural addressing, fault, and ABI specification for
  revision 0.11. [`isa.html`](isa.html) is the self-contained visual encoding reference. Neither
  document defines Cache or fitted-system policy.
- [`hardware-architecture.md`](hardware-architecture.md): current Stage 12 core, FPU, fetch, and
  cache microarchitecture, including instruction latency and implementation timing context.
- [`cpu_v3_structure.puml`](cpu_v3_structure.puml): maintainable source for the CPU V3 structure
  diagram.

The complete Tang Nano 20K composition is documented in
[`systems/cpu-v3-tang-nano-20k/docs/README.md`](../../../systems/cpu-v3-tang-nano-20k/docs/README.md).

## Version boundary

ISA revision and implementation Stage are independent. Revision 0.11 defines the architectural
instruction and ABI contract. Stage 12 names the current microarchitecture optimization level and
does not change the ISA encoding.

## Implementation owners

Public type names remain available through the existing IP modules. Source
ownership follows state and physical ports:

| Owner | Responsibility |
| --- | --- |
| `rcc_backend/mod.rs` | Reachability, optimization/allocation and compilation orchestration |
| `rcc_backend/abi.rs`, `lowering.rs` | Target register convention and symbolic instruction selection |
| `rcc_backend/linker.rs`, `program.rs` | Branch relaxation, image/debug layout, validation and result/error types |
| `hardware/cache/storage.rs` | Parity-split data BSRAM, tags, valid/victim and dirty metadata leaves |
| `hardware/cache/instruction.rs`, `data.rs` | Read-only instruction/refill owner and independent write-allocate/maintenance owner |
| `hardware/fpu/execution.rs` | Selected RF/compute calendar and instruction state |
| `hardware/fpu/frontend.rs`, `register_file.rs`, `scalar_alu.rs`, `multiplier.rs` | Shared frontend, architectural/LUT state, arithmetic and physical multiplier boundary |
| `hardware/fpu/standalone.rs` | Independent standalone path implementations used for qualification |

Cache geometry/address helpers remain with the cache module. FPU instruction
encoding and special-function port contracts have explicit homes; selected
execution does not call a standalone scalar model to evaluate its ALU. Unit and
co-simulation harnesses live in separate test files beside their owners. These
boundaries preserve instruction/CB formats, memory layout, physical module
identities, cycle behavior and independent oracles.

## Co-simulation replay files

The production FPU owner replay retains every input beat, CE pause and abort
age. Every beat checks the public outputs, accumulator and all 68 observed RF
words; a local Verilog loop compares a packed expected snapshot instead of
generating one assertion statement per word per beat. The selected execution
RF, multiply and special-function leaves and the end-to-end core regressions
remain separate checks. Standalone scalar, vector and special-function paths
have their own bounded qualifications; the selected product calendar is checked
through the actual execution owner.

SINCOS numerical accuracy is checked once by `sincos_full_range_accuracy`: its
dense sweep covers every raw input in ±45 radians, including the ±2π rotation
domain and quadrant neighbours, with the same four-LSB bound. Signed/exponent
edges, the full-i32 stride and fixed-seed random inputs extend that check.

Core emulator/RTL co-simulations retain each generated project, `tb.v` and
`sim.vvp` in the worktree's `target/core-cosim-<pid>-<counter>/`, including after
successful runs and failures in the caller's trace comparison. No keep switch
is required. Each run atomically creates its directory and skips existing names,
including those retained from a previous process with the same PID. Other
directory-creation errors fail the run. Replay a retained executable with
`vvp sim.vvp` from its directory;
use `VVP_EXE` when the simulator is not on `PATH`. Run `cargo clean` from the
task worktree when final task cleanup is authorized.

The ignored `core_cosim_retains_replay_after_comparison` regression checks a
real bounded emu/RTL run, then deliberately mismatches the expected halt signal
and catches the comparison panic. That expected failure checks replay lifetime;
it does not represent an RTL simulation failure.
The ordinary `core_cosim_directory_collision_preserves_existing_replay` test
prepares an old directory and sentinel, reuses its PID/sequence candidate, and
checks that allocation leaves the old contents unchanged and selects a fresh
directory. This filesystem regression does not invoke an RTL simulator.
