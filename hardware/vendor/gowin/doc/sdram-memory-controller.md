# SDRAM memory controller combination

The vendor crate owns the shared arbiter, SharedSdramPort adapter, related-clock
64/32-bit gearbox and native SDRAM controller in `src/sdram_memory_controller`.
The CPU system consumes these through its normal vendor dependency and compatibility
exports. GPU v2 consumes the combination through a dev dependency for validation.
The vendor never depends on GPU, CPU or system crates.

## Current validation

The Rust service oracle and calibration experiments are executable, and the GPU
frontend oracle consumes their real DMA responses. System all-targets compilation
checks the relocated dependencies. Six relocated Verilog sources match the previous
HEAD after line-ending normalization; module identities and generated source paths
are preserved. `RtlSources` provides the authoritative bundle used by the target.
This migration does not enable the production bridge's continuation inputs.

The independent cycle emu and reusable connected RTL fixture are separate work.
The existing SharedSdramPort still declares `EMU_AVAILABLE=false`. The Rust timing
study is an event approximation with estimated bridge transports, not that emu or
a pin-level qualification. No CPU tests, system co-simulation, PnR or board test is
claimed by this GPU Rust validation. No DSP/BSRAM/SSRAM resource ledger is applied.

## Host service and data boundary

`ports::Service` exposes `submit(client, request)`, `step()`, `cycle()` and `idle()`.
Submit accepts a bounded host queue entry and returns a logical ID; it is not a
physical request handshake or a hardware transaction tag. Events are Started,
ReadBeat and Complete. Read beat indices are increasing, last identifies exactly
the final beat, and every logical request completes once. Read events cannot be
backpressured at this facade: callers reserve receive capacity before submission.
Low-level emu/RTL must model its actual ready/valid and receive buffering separately.

Addresses are bytes; system beats are 64-bit little endian. Naturally aligned
32/64/128 B requests are native GPU transactions. A naturally aligned 512 B request
is a logical group of four 128 B sectors, not a new native burst. Writes carry
one enable bit per byte. Invalid length/alignment/image range requests fail before
consuming an ID. Actual system scalar and mask RTL interfaces remain unchanged.

OracleWord and OracleImage have private fields. `OracleWord::constant::<VALUE>()`
and `OracleImage::filled::<BYTE>(base, len)` construct constant data safely.
Runtime host initialization uses explicitly unsafe `from_host(..., source)`.
This denotes responsibility for an external stimulus boundary; no unsafe memory
operation is performed. It does not waive bounds checks or prove data provenance.
It must not be used to inject a host-computed intermediate into a closed audited
frame. Safe memory reads produce Memory-origin words; host bit observation is
allowed for the oracle and independent scoreboard.

This transport type is distinct from audited::Fixed. The audited and GPU library
unsafe restrictions remain intact. Counted computations still obtain values
through checked Model input storage and frame reads, never a new Fixed constructor.

## Modes and functional visibility

| Rust implementation | Purpose | Background handling |
| --- | --- | --- |
| `sim::average::Memory` | Reproducible performance experiments with a fixed Profile | Already included in the profile; never injected a second time |
| `sim::oracle::Memory` | Functional service with explicit configured contention | Generates periodic traffic and runs event-level arbitration/controller timing |
| `sim::calibration::analyze` | Measure service means and derive a Profile | Same controller/traffic assumptions, bounded synthetic experiment |

The fixed service has an ordered foreground FIFO. It applies per-class first-read
or write-completion offsets and adds its own queue waiting. A 512 B read profile
has four sector-first offsets so calibrated interruptions are retained. Writes
commit atomically at the configured completion; this is functional visibility,
not the native write pin sequence.

Configured service arbitrates Display first, then circular first-ready among
Instruction, Data, DMA, GPU read-only, framebuffer read and framebuffer write.
It retains a logical group across possible sector breaks and emits Started once
and Complete once. Writes become visible for each issued run at its completion.
Row preparations and native data timing are computed when a run is granted.
Later interactive host arrivals cannot undo an already planned run; this event
model is therefore not a replacement for the independent cycle protocol emu.

Configured background addresses must be disjoint from the supplied foreground
image. Those requests model service contention only; they do not populate a
second CPU/display functional memory. `idle()` means all foreground work completed,
even if periodic background work remains. Cycle and total foreground/background
request limits stop overload; the oracle does not promise an unconditional fairness
bound under unlimited strict-priority Display demand.

## Configuration and assumptions

| Configuration | Meaning |
| --- | --- |
| Controller transports | Request, read-return and write-ack estimates in core cycles |
| Refresh interval / continuation age | Refresh admission and chain-age bounds |
| ChainPolicy | ExistingUnchained or explicitly labelled ChainedCandidate |
| Stream client, period, phase | Request class and deterministic arrival schedule |
| Stream bytes, write_every | Burst size and read/write mix (0 means read only) |
| Stream base, span, stride | Disjoint address region and locality sequence |
| Stream batch | Same-time requests per period, preserving declared bandwidth |
| Limits / calibration bounds | Maximum cycles, requests, samples and queue work |

No random jitter is added. Identical configuration and trace produce identical
results. Load presets are estimates: Display uses the bandwidth of 400x240 RGB565
at 60 Hz, a 32 B read every 150 system cycles. Batch 50 uses fifty such requests
every 7500 cycles. CPU estimates are an instruction read every 1728 cycles and
a data transaction every 288 cycles, with every third data transaction writing.
These are user-adjustable assumptions, not measured CPU or scanout traces.

The timing study uses the integrated BANK_BIT=5 geometry: bank=byte[8:7],
row=byte[22:12], four banks sharing one native DQ bus. Other-bank PRE/ACT can hide
inside a current burst. Same-bank row conflicts, refresh, late preparation and
Display interruption can leave unhidden delay. Crossing a bank alone has no fixed
penalty. Default column timing follows the current CL2/RCD2/RP2/RFC9 controller;
the bridge transports remain coarse explicit estimates.

## Calibration and reproducibility

Calibration permits one logical foreground at a time, excluding its own backlog
from measured service means. CPU/display contention and refresh remain included.
Foreground self-queueing is reported separately and is added by the fixed runtime;
it must not be charged twice. All clients still pass through one physical service.
Each class records first/completion means, p95 and min/max completion, arbitration
and self-queue waits, row state, bank changes, hidden preparation, refresh waits,
chain sectors and chain-break causes. Core counters retain core units.

Profile generation requires samples for all eight direction/size classes, fails
for empty buckets, and rounds means upward once. It validates sector ordering.
Compare Solo/CpuOnly/DisplayOnly/Both using the same foreground trace; nonlinear
contention means separate CPU/display increments need not sum to the Both result.
ExistingUnchained is the production configuration; ChainedCandidate only studies
the controller's continuation potential until gearbox/arbiter integration is tested.

GPU-owned reproduction and tests are documented in
[GPU SDRAM integration](../../../../ip/gpu-v2/docs/sdram-memory-controller.md).
The bounded probe writes calibration.csv and profiles.txt under the selected target
directory. Preserve those reports with the source/configuration used to create them;
they are estimates, not physical bandwidth evidence.
