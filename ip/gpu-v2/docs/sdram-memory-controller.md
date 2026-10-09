# SDRAM memory controller integration

## GPU burst transport

`memory::ports::MemoryPort` is the GPU-owned cycle interface for a single
outstanding, naturally aligned 128-byte read or write. Addresses are bytes;
there are sixteen ordered 64-bit beats. Request acceptance, write-beat
acceptance, read-beat delivery and terminal success/error are separate fields.
The last write beat never substitutes for the controller's write ACK. An error
may terminate early; already written bytes are not rolled back. The caller
retains ownership until terminal completion and clocks accepted work during
compute CE stalls or a fault drain.

A read requires all sixteen destination credits before admission. A write
requires its first beat before presenting the descriptor and a reserved source
for continuous remaining data. The current physical MC has no arbitrary
mid-burst write-valid stall. Optional write input permits initial preparation
and expresses source availability; it does not promise such a physical stall.
Blocked descriptors/data stay stable. A `Result::Err` denotes adapter/caller
protocol failure; `Response::complete=Some(false)` denotes a memory error.
Reset/cancel is deliberately absent: system reset must drain before resetting
this ownership state. Four serial requests form a 512-byte tile plane; this
interface does not implicitly enable group mode.

`tests/support/sdram/burst.rs` maps this interface directly to the existing
serial `emu::Combination`, with no Service host queue or latency estimate.
It checks address/image bounds, converts byte addresses to halfword addresses,
uses framebuffer read/write clients, counts actual handshakes and forwards the
terminal response. It stores counters/held control, not a second burst payload.
`memory_burst` independently checks four-bank write/read data and guards,
the real write-data-to-ACK gap, first-beat reservation, accepted source underrun,
stable blocked inputs,
address rejection and second-request rejection. These bounded Rust adapter
tests reuse the previously co-simulated MC; they are not new adapter RTL,
framebuffer integration, concurrent-client qualification or reset-drain proof.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label gpu-memory-burst -CargoArgs @('-p','gpu-v2','--test','memory_burst')
```

## Existing oracle and texture adapters

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

The frontend DMA cover adapter still issues one cover at a time. Group
execution inside the memory service does not make the whole GPU frontend a
cycle-accurate implementation. The CPU V3 board enables one-slot early
admission and keeps the indivisible group mode off. Physical SRAM/Flash-reload
group measurements are recorded in the
[board study](../../../hardware/vendor/gowin/doc/sdram-traffic-probe.md#group-board-results-2026-10-02).

## Cycle service contract

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

## Integration configuration

The host `ports::Service` accepts byte-addressed 32/64/128/512 B jobs and
reports Started/ReadBeat/Complete events. Its queue acceptance is distinct
from a hardware arbiter grant. `emu::service::Memory` runs the cycle-executed
arbiter, adapter, bridge and pin model. `Combination::new` and
`combination::rtl_sources` retain the serial standalone default; the
`with_options` variants select early admission and indivisible groups
explicitly. Keep emu and RTL options identical.

| Signal or option | Standalone serial default | CPU V3 board |
| --- | --- | --- |
| Arbiter `lookahead_enable` | Low | Adapter `cpu_lookahead_window` |
| Adapter `controller_stream_active` | Low | Bridge `stream_active` |
| Reserved address | Unused | Adapter to bridge `next_valid/address` |
| `EARLY_GRANT` / `PREPARE_NEXT` | 0 / 0 | 1 / 1 |
| `CHAIN_GROUP_FOUR` | 0 | 0 |

The CPU V3 board grants at most one next descriptor while the active line
stream continues. Its physical controller may prepare another bank, and the
next request can enter the bridge on accepted final-response retirement.
The current write payload and response owner do not change on early grant.
The reserved request is irrevocable; Display priority applies before its
grant. See the
[vendor contract](../../../hardware/vendor/gowin/doc/sdram-memory-controller.md#optional-early-admission)
for alignment, backpressure and timing constraints.

Cycle/pin RTL comparison covers read/write data and guards, reset, refresh,
row conflicts, held final responses and illegal descriptors. The CPU V3
production project also passes 54/108 MHz PnR and full-system Icarus boot
simulation. The standalone board-probe results in the
[traffic study](../../../hardware/vendor/gowin/doc/sdram-traffic-probe.md)
do not qualify this CPU V3 bitstream on physical hardware.
