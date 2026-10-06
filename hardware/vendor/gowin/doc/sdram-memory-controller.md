# SDRAM memory controller combination

The vendor crate owns the shared arbiter, SharedSdramPort adapter, related-clock
64/32-bit gearbox and native SDRAM controller in `src/sdram_memory_controller`.
The CPU system consumes these through its normal vendor dependency and compatibility
exports. GPU v2 consumes the combination through a dev dependency for validation.
The vendor never depends on GPU, CPU or system crates.

## Current validation

The Rust service oracle and calibration experiments are executable, and the GPU
frontend oracle consumes their real DMA responses. System all-targets compilation
checks the relocated dependencies. Module identities and generated source paths
are preserved. `RtlSources` provides the authoritative bundle used by the target.
Production continuation remains off; independent configurations exercise early
admission and explicit four-sector groups below.

The independent `emu::Combination` now executes arbiter, adapter, two-entry pair
gearbox and rising-capture native controller state on related clock edges.
SharedSdramPort exposes its own executable Module emu. `combination::rtl_sources`
exports the connected standalone RTL using the authoritative components. GPU tests
compare every observable logic-clock handshake against the real controller and pin
model, including scalar lanes, refresh, invalid requests, reset and response waits.
The approximate timing study remains separate from this cycle implementation.
The standalone [traffic probes](sdram-traffic-probe.md) have passed PnR. The serial
baseline has physical measurements; updated early/group qualification is pending
usable UART data. CPU tests/system co-simulation were not run in this unit.

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
| `emu::service::Memory` | Functional host facade over independent cycle handshakes | Explicit live client submissions, arbitrated by the production arbiter state |

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

## Independent cycle protocol boundary

One logic tick advances two controller rising edges and their intervening transport
and device phases. The integrated BANK_BIT=5, CL2/RCD2/RP2/RFC9, open-row,
request-pipeline and rising-capture profile is fixed; initialization length is
configurable. This is a protocol model, not an analog or arbitrary-clock SDRAM model.
The cycle engine does not import sim::average or sim::controller.

Intermediate line read beats cannot be stalled by the existing RTL; consumers must
reserve the whole native response. Scalar and final line responses remain stable
until accepted. A write may wait before its first payload, but once DQ streaming
starts its supply must be continuous. The cycle engine detects underrun; the host
facade reserves the complete payload. Arbitrary per-byte burst masks are rejected
before ID acceptance. Native scalar halfword lanes remain supported through the
low-level DMA port, and host DMA bursts are serialized into those scalar operations.

The host facade splits 512 B into four real 128 B transactions. In the default
configuration, Display arrivals can intervene at each sector boundary. The optional
early-grant configuration makes one successor irrevocable before that boundary;
Display retains priority only until the successor is granted. No linked physical
512 B continuation is claimed by either configuration. The separately enabled
four-sector group below provides physical chaining through a different admission
contract; it is not an automatic extension of a per-sector grant.

Cycle reset aborts queued and active host IDs while retaining the memory image and
already clocked writes. IDs remain monotonic across reset. Configuration bounds
queue capacity, lifetime submissions and total logic clocks; external fixture
processes additionally have a wall watchdog.

GPU-owned reproduction and tests are documented in
[GPU SDRAM integration](../../../../ip/gpu-v2/docs/sdram-memory-controller.md).
The bounded probe writes calibration.csv and profiles.txt under the selected target
directory. Preserve those reports with the source/configuration used to create them;
they are estimates, not physical bandwidth evidence.

## Optional early admission

`Combination::with_early_grant`, `combination::rtl_sources_with_early_grant` and
`emu::service::Config::early_grant` enable the same one-slot protocol. The
reusable combination remains serial by default. The CPU V3 full-system board
build enables `EARLY_GRANT=1` on `SharedSdramPort` and `PREPARE_NEXT=1`
on the 54/108 MHz bridge, connecting the adapter's lookahead and reserved
address to the seven-owner arbiter and native controller. The board PLL lock
also conditions the system reset. This is one reserved successor per active
request; it does not turn four 128 B sectors into an indivisible transaction.

The adapter opens its successor slot in the last four logic beats of an active
line while the native stream is active. The arbiter records a reserved owner and
the adapter records its descriptor. Grants advance the non-display cursor; a later
Display arrival cannot revoke a grant. No new grant occurs on a terminal response
edge. LAST/error retirement promotes the reserved owner, with no second reservation
until the slot is released. Reset aborts both slots. Native initialization loss
requires a shared upstream reset, as in the board wrapper.

Active write payload selection uses its owner, independently of the next
request's direction/length. A same-client next descriptor may change address or
read/write mode while its payload still describes the active segment. Only accepted
LAST promotes payload/response indices. A promoted line write has one source-prime
cycle. Scalar writes are excluded from early grants because their request-edge
payload shares the current stream bus. Scalar/final read responses remain stable
under backpressure; intermediate line beats still require a reserved sink.

The gearbox transports a stable next address to the core. `PREPARE_NEXT` can open
a closed *other* bank while the current DQ stream continues, subject to refresh
age and tRCD/tRP. It never precharges the active bank or an already open conflicting
row in this mode. CHAIN/READ_CHAIN remain zero: every segment still has its own
native completion, adapter LAST and remaining transport gaps.

## Optional four-sector physical groups

`emu::service::Config::chained_groups`, `Combination::with_options` and
`combination::rtl_sources_with_options` enable `CHAIN_GROUP_FOUR` in both adapter
and gearbox. Defaults remain off. In that configuration alone, line-count code
2 means one naturally aligned 512 B GPU group; the default still rejects code 2.
The existing six-bit native word-count wire uses zero as its opt-in group sentinel.
The adapter expands it to 64 logic beats, and the gearbox issues four physical
32-word READ or WRITE segments. A group has one arbiter grant and one final
LAST/write ack. It does not expose four separately arbitrated completions.

Display wins before admission. Once admitted, all four segments are irrevocable;
later Display work waits for the group, although early admission may reserve its
tail slot. Only one successor transaction may be reserved. This changes the
priority boundary and can increase another client's maximum waiting time. It
does not promise fairness under unlimited Display demand.

The gearbox snapshots the stable descriptor into the 108 MHz domain before core
admission. This is an explicit pipeline with normal related-clock timing checks.
Group alignment turns sector addressing into wiring. Two 64-bit write entries
carry the continuous stream; no 512 B payload RAM is added. The host facade owns
the complete payload and response sink before submission. Initial source delay
is allowed; source underrun during DQ streaming is an error. Intermediate read
beats require an always-ready sink; final responses retain the adapter's hold
semantics. Reset aborts the group and any reserved successor without rolling back
already clocked writes.

The native core enables CHAIN/READ_CHAIN only for the group wrapper. Its internal
next descriptor is valid only while another group segment remains. Other-bank
PRE/ACT may overlap current DQ, subject to bank timing and refresh. A qualified
continuation issues the next column command exactly 32 native clocks after the
previous one. A failed qualification stops the stream and restarts the remaining
segments with normal row/refresh delays, retaining group ownership. Completion
is published only after the fourth segment. Groups are bounded to four segments;
the refresh-age continuation gate prevents indefinite refresh postponement.

Independent functional-oracle comparisons check all sizes and memory guards.
Cycle/RTL tests cover read/write, conflicting rows, refresh fallback, Display
before/after admission, alignment rejection, source underrun and midstream reset.
The pin fixture separately counts accepted chains and checks physical column
intervals. Standalone resource/timing and board status live in the
[probe qualification](sdram-traffic-probe.md#four-sector-group-probe).

## Cycle-derived average Rust oracle

`sim::cycle_calibration::analyze` measures the actual independent cycle facade
with serial, early admission or explicit groups and validated `traffic::Load` streams.
Initialization is excluded. Background reads/writes use real arbiter clients and
an isolated pin-memory region; overlap with foreground data is rejected. Idle
refresh and bank/row state continue across samples. There is one eligible foreground
job at a time; its backlog is reported separately and excluded from service means.
Cycle, sample, queue and submission bounds terminate overload.

The report records first/completion means, p95, grant wait and four 512 B sector
first offsets. `Report::profile()` rounds each mean upward once; missing classes
fail. `average::Memory` then provides the functional oracle with stable offsets
and its own FIFO wait, without re-adding background delay or running the cycle
engine per runtime access. `Profile::gpu_early_grant(load)` is the bounded default
calibration helper; `Profile::gpu_chained_groups(load)` selects physical groups.
Functional expected bytes are checked independently of the cycle engine; it
supplies edge-by-edge protocol expectations for RTL co-simulation. The configured event oracle and its
ideal `ChainedCandidate` study remain separate from both implemented configurations.

The GPU example `sdram_early_grant_model_probe` compares 128 samples per class for
Solo/CPU/Display/Both/Batch50. The representative trace mixes bank and row locality;
these averages depend on its addresses and phases, rather than being latency
promises for every workload. Current model completion means (54 MHz logic clocks):

| Estimated load | Serial read512 | Early read512 | Group read512 | Serial write512 | Early write512 | Group write512 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Solo | 91.97 | 89.16 | 72.41 | 93.11 | 91.30 | 71.71 |
| CPU | 96.54 | 93.44 | 73.00 | 98.19 | 96.61 | 72.80 |
| Periodic Display | 98.30 | 94.81 | 72.73 | 100.33 | 98.06 | 72.84 |
| CPU + periodic Display | 103.40 | 99.18 | 73.89 | 105.82 | 103.30 | 73.79 |
| CPU + batch50 Display | 110.04 | 100.38 | 74.22 | 103.20 | 103.41 | 74.03 |

These are cycle-model averages, not physical-board measurements. Changed arbitration
phase can worsen a class even if group throughput improves. Early admission removes
some dispatch and bank preparation delay; it does not remove every physical gap.
Groups additionally remove per-sector arbitration/dispatch, but change the
irreversible priority boundary. Evidence is under `target/gpu-v2-sdram/chained-model/`.
