# SDRAM memory controller integration

The shared combination belongs to the Gowin vendor crate, consumed here only via
dev dependency. Its ownership, host data boundary, traffic configuration and timing
assumptions are defined in the [vendor contract](../../../hardware/vendor/gowin/doc/sdram-memory-controller.md).

The GPU frontend owns MemoryPort. `frontend::sim::oracle::run_with_memory` receives
exactly the requested DMA words through that port; the existing `run` remains a
direct-image functional fixture. GPU library code does not import the vendor.
The generic `tests/support/sdram::Adapter<S: Service>` connects the vendor facade,
and `emu::service::Memory` now uses the same input scenario and scoreboard.

The adapter covers eight-byte-aligned DMA ranges with naturally aligned native
requests, selects only requested words and checks ID/beat/last/completion order.
For example, a 40 B DMA starting eight bytes into a 32 B line reads a 64 B native
cover but sends only five words to the scratchpad. The cover must actually exist
in the supplied image; a missing tail fails rather than inventing zero data.
Accepted reads complete before the next cover request is admitted, preventing an
error in a later request from orphaning earlier accepted work.

The Rust combination tests compare the frontend's scratchpad/vertex outputs with
the independent direct-image oracle in fixed-average and configured-load modes.
They verify untouched input bytes and real response data. The same frontend
program also runs against the independent cycle combination. Existing frontend
timed reservations retain their explicit DMA latency fixture; this adapter does
not implement numerical per-cycle GPU execution.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label gpu-sdram -CargoArgs @('-p','gpu-v2','--test','sdram_memory_controller','--test','sdram_integration')
& scripts/run-cargo.ps1 -Subcommand run -Label gpu-sdram-probe -CargoArgs @('-p','gpu-v2','--example','sdram_memory_probe','--','target/gpu-v2-sdram')
```

The probe compares Solo/Display/CPU/Both and a batched Display load for both chain
policies, each with 1024 bounded foreground samples. Reports stay under target;
raw mean values and upward-rounded profiles are separate. `sdram_emu_rtl` additionally
checks live Display arrivals, real sector boundaries, masks, guards, reset and
source underrun. Its explicit Icarus tests compare every logic edge of the
connected combination and run all nine board workloads with UART CRC decoding:

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label gpu-sdram-cycle -CargoArgs @('-p','gpu-v2','--test','sdram_emu_rtl')
& scripts/run-cargo.ps1 -Subcommand test -Label gpu-sdram-pin -CargoArgs @('-p','gpu-v2','--test','sdram_emu_rtl','--','--ignored','--nocapture')
```

The pin fixture uses a 10.102 ns controller period, within the model's CL2 tCK
minimum, and scales both related clocks together. It proves cycle/protocol
alignment, not the fitted 108 MHz die timing. The independent
[board probe](../../../hardware/vendor/gowin/doc/sdram-traffic-probe.md) measures
actual 54/108 MHz service after the user enables physical-board testing.

The optional early-grant facade is enabled with `service::Config { early_grant:
true, ..Default::default() }`. A 512 B job advertises its next sector while keeping
the active payload index until LAST. Only one successor is admitted. Independent
functional-oracle comparisons cover read/write data, guards and all four sizes;
cycle/RTL transcripts also exercise mixed-direction same-client descriptors,
Display before/after a grant, illegal successors, final sink stall and double-slot
reset. Six explicit Icarus tests cover serial, early and grouped cycle/board fixtures.

For stable performance modelling, calibrate `sim::cycle_calibration` using the
chosen early-grant flag and validated CPU/Display streams, then give its `profile()`
to `average::Memory`. This retains measured mean sector offsets rather than ideal
chaining. `average::Profile::gpu_early_grant(load)` supplies the default trace.
The configuration, statistical boundaries and current model means live in the
[vendor contract](../../../hardware/vendor/gowin/doc/sdram-memory-controller.md).

```powershell
& scripts/run-cargo.ps1 -Subcommand run -Label gpu-sdram-early-model -CargoArgs @('-p','gpu-v2','--example','sdram_early_grant_model_probe','--','target/gpu-v2-sdram/early-model')
```

Enable `service::Config { early_grant: true, chained_groups: true,
..Default::default() }` for an aligned 512 B GPU job admitted as one indivisible
four-sector group. Its read/write stream is physically chained when bank/refresh
timing permits, with normal restarts otherwise. This changes Display's preemption
boundary; see the [vendor group contract](../../../hardware/vendor/gowin/doc/sdram-memory-controller.md#optional-four-sector-physical-groups).
`average::Profile::gpu_chained_groups(load)` calibrates that exact cycle path once
and returns stable average offsets. Functional data goldens remain independent.

The unchanged frontend DMA cover adapter still issues one cover at a time.
Group execution inside the memory service does not make the whole GPU frontend
a cycle-accurate implementation. Production system activation remains separate;
the group mode defaults off. Physical SRAM/Flash-reload group measurements are
recorded in the [board study](../../../hardware/vendor/gowin/doc/sdram-traffic-probe.md#group-board-results-2026-10-02).

## Shared baseline for module worktrees

Consumers use `ports::Service` (`submit`, `step`, `cycle`, `idle`) and byte-addressed
32/64/128/512 B requests. Host acceptance is a queue entry, not the arbiter grant;
reserve the whole payload/read sink before submission and observe real events.
`emu::service::Memory` executes the independent cycle combination. Its default
`Config` is serial; `early_grant: true` enables one reserved successor, and
`chained_groups: true` admits aligned GPU 512 B jobs as indivisible physical groups.
The public source bundle uses the same options for RTL validation.

For deterministic experiments, calibrate that exact `Config` and `traffic::Load`
with `sim::cycle_calibration::analyze`, then use the resulting Profile with
`sim::average::Memory`. The `gpu_early_grant`/`gpu_chained_groups` helpers select
their documented representative traces; their means are not universal latencies.
The older ideal `ChainedCandidate` study does not select physical hardware.
Sampling/texture's 128 B `GpuReadOnly` requests stay 128 B in every configuration:
they must not claim the framebuffer's four-sector chaining bandwidth benefit.
Do not replace the sampling numerical golden or confuse its functional cache
with a cycle-executed cache controller.

## Offline MC source integration

The host `ports::Service` contract and external data boundary remain compatible
with `fb472c9`: byte addresses, 64-bit beats, bounded queue acceptance and
Started/ReadBeat/Complete events. The independent cycle/RTL implementation first
appears in `3b69ba5`; `d11abf2` records its historical serial board results.
A worktree based only on `fb472c9` must first take those two commits before the
commit containing the optional early/group changes in this section. The audited
logic certificate commits are separate dependencies. Do not copy individual RTL
files or substitute the mean-latency oracle for this cycle implementation.

Use `emu::service::Memory::new(image, service::Config { ..Default::default() })`
for the serial cycle baseline. `Combination::new(image, init_cycles)` and
`combination::rtl_sources(init_cycles)` select the same serial protocol. The
explicit `with_options` / `rtl_sources_with_options` arguments are
`(image, init_cycles, early_grant, chained_groups)` and
`(init_cycles, early_grant, chained_groups)`, respectively. Sample with identical
options in emu and RTL; the board PLL/pads are outside the standalone source bundle.

| Low-level addition | Serial binding | Experimental binding |
| --- | --- | --- |
| Arbiter `lookahead_enable` | Tie low | Adapter `cpu_lookahead_window` |
| Adapter `controller_stream_active` | Tie low | Gearbox `stream_active` |
| Adapter `controller_next_valid/address` | Unused | Gearbox `next_valid/address` |
| Adapter `EARLY_GRANT`, `CHAIN_GROUP_FOUR` | Both zero | Match service options |
| Gearbox `PREPARE_NEXT`, `CHAIN_GROUP_FOUR` | Both zero | Match service options |

The production CPU wrapper retains the serial ties/defaults. Its dependency
compiles, but this offline delivery is not a new whole-system co-simulation,
PnR, CDC or board qualification. Existing GPU evidence covers six explicit
Icarus edge/pin tests, independent cycle/oracle data comparison, reset, refresh,
row conflicts, held final responses, source underrun and illegal admissions.
The additional bounded active-display test and its narrow startup margin are
documented only in the [probe study](../../../hardware/vendor/gowin/doc/sdram-traffic-probe.md#bounded-active-demand-cycle-check).
The additional [production-display component check](../../../hardware/vendor/gowin/doc/sdram-traffic-probe.md#bounded-production-display-and-cdc-check)
executes the real four-row buffer, publication/release CDC and RGB consumer with
the connected vendor RTL and pin model. Its finite active-region pass is separate
from complete system qualification. The group image passes repeated physical
SRAM and verified Flash software-reload measurement; early-only board measurement
and full power-off cold-start UART remain open.
