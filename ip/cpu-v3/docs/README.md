# CPU V3 IP documentation

These documents describe the reusable CPU V3 ISA and processor IP. Board-specific boot, SDRAM,
display, device allocation, and fitted timing belong to the Tang Nano 20K system documentation.

## Documents

- [`isa.md`](isa.md): normative ISA, architectural addressing, fault, and ABI specification for
  revision 0.9. [`isa.html`](isa.html) is the self-contained visual encoding reference. Neither
  document defines Cache or fitted-system policy.
- [`hardware-architecture.md`](hardware-architecture.md): current Stage 12 core, FPU, fetch, and
  cache microarchitecture, including instruction latency and implementation timing context.
- [`cpu_v3_structure.puml`](cpu_v3_structure.puml): maintainable source for the CPU V3 structure
  diagram.

The complete Tang Nano 20K composition is documented in
[`systems/cpu-v3-tang-nano-20k/docs/README.md`](../../../systems/cpu-v3-tang-nano-20k/docs/README.md).

## Version boundary

ISA revision and implementation Stage are independent. Revision 0.9 defines the architectural
instruction and ABI contract. Stage 12 names the current microarchitecture optimization level and
does not change the ISA encoding.

## Co-simulation replay files

The production FPU owner replay retains every input beat, CE pause and abort
age. Every beat checks the public outputs, accumulator and all 68 observed RF
words; a local Verilog loop compares a packed expected snapshot instead of
generating one assertion statement per word per beat. The selected execution
RF, multiply and special-function leaves and the end-to-end core regressions
remain separate checks. The standalone ProductPath implementation remains a
reference, without a migration comparison against the older separate issuers.

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
