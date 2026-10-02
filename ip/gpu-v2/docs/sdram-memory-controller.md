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
