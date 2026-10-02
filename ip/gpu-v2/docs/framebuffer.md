# Framebuffer reference and bounded port model

The materialized-surface reference implements all eight unsigned D16 comparisons,
independent depth write, linear RGB565 REPLACE/SRC_OVER, and coverage masking.
SRC_OVER expands RGB565 by bit replication, rounds the weighted RGB8 sum divided
by 255, then quantizes each channel to 5/6/5 with round-to-nearest. The denominator
is odd, so no halfway tie occurs. Every overlapping write quantizes again. Zero
alpha retains the old color code but may still write depth. No destination alpha,
alpha test, prefetch, framemeta allocation, fast clear, or row dirty optimization
is implemented. `MaterializedSurface` deliberately separates valid external
content from the cache's per-plane dirty state.

`sim::bounded` executes a finite control/port calendar and calls the oracle for
pixel arithmetic. It is **not audited counted/timed arithmetic**, an independent
arithmetic emulator, or RTL. The oracle call's result availability is an explicit
schedule assumption; there is no certified multiplier, rounding, or packing
latency. This component is not connected to the production GPU/system.

## Data and maintenance contracts

Four true dual-port 1024x16 banks hold eight 16x16 tiles, with color in each bank's
low 512 words and depth in its high 512 words. For plane `p`, line `l`, and tile
coordinates `x,y`, `bank=(x&3)^((y&1)<<1)` and
`address=p*512+l*64+y*4+(x>>2)`. Every per-cycle access is checked against bank-port
overbooking and same-address dual-port collision. Aligned four-pixel row groups
and aligned 2x2 quads each visit all four banks. Little-endian 64-bit beat word
`i` is routed using its pixel coordinates, including the odd-row bank permutation.

External planes are disjoint, 512-byte-aligned tile-linear allocations. Bounds
are multiples of 16, at most 512x256; plane bases are byte addresses. A tile uses
512 bytes per plane; its row-major word offset is `(y&15)*16+(x&15)`.
The system [layout](../../../systems/cpu-v3-tang-nano-20k/src/layout.rs) uses
16-bit **word** bases instead; its adapter must multiply addresses by two.
Each tile plane is four independent, ordered 128-byte requests. No MC 512-byte
group is requested.

There is one maintenance transaction and one 64-bit write skid register. B-port
writeback capture occurs one cycle before the captured beat can be offered;
accepted writes capture the next beat on that cycle. The skid holds stable under
backpressure. Read returns are never backpressured: the complete destination
line is reserved before submission. Both planes must refill successfully before
the new tag is published. A dirty plane is cleared only after all four terminal
success ACKs. Victim tags remain unchanged until all dirty planes are acknowledged;
the final accepted write beat is not an ACK. Flush completes only after all queued
ROP work and required writebacks succeed.

The common GPU-owned `memory::ports::MemoryPort` returns request acceptance, individual
write-beat acceptance, indexed read beats, and a separate terminal success/error.
Exactly 16 beats precede a successful terminal response; an error may terminate
early and never publishes a partial refill or clears a dirty plane. Single
outstanding ordering provides implicit identity, retained until the terminal event.
A presented request remains stable until accepted. Computation CE never gates
the memory port or B maintenance. Fault cancels work not yet presented, keeps an
already presented request stable through acceptance, drains its traffic, then discards
pending output/ROP; successfully completed earlier writes are not rolled back.
Fault recovery/retry and full render/display ownership remain outside this model.

`sim::fixture` is a deterministic, byte-backed serial service with programmable
request/beat stalls, delayed ACK and terminal failure injection. Its wall cycles
are **not MC latency or performance measurements**. The independent real-cycle
MC comparison below uses the common adapter; production integration remains future work.

## ROP calendar and dependencies

The output store has two slots, eight 32-bit logical rows each: RGBA8888 followed
by zero-extended D16 for each of four lanes. Lane order is (0,0), (1,0), (0,1),
(1,1). Headers contain even x/y and mask4. One final write is accepted per enabled
cycle. Row order and reserved bits are checked; row 7 publishes the record. A
partial record is never visible. Slots are released only at `rop_committed`.
Context is immutable for the entire model lifetime.

One active quad is deliberately serialized; no overlap forwarding is assumed.
Every row read has a one-enabled-cycle return register. Cache A reads likewise
capture data on the issuing edge, usable only by a later phase. CE holds all ROP
state and return data. The following is the implemented hot path after the one
tag-lookup/start cycle; a cache miss adds maintenance and another lookup.

| Phase | Output read issue/return | Four-bank A operation | Numerical dependency |
| --- | --- | --- | --- |
| 0 | Issue RGBA lane 0 | Read covered depth | Depth available next phase |
| 1 | Issue D lane 0; return RGBA 0 | Read covered color | Color available next phase |
| 2 | Issue RGBA 1; return D 0 | Idle | Oracle lane 0, with both old values |
| 3 | Issue D 1; return RGBA 1 | Idle | Hold lane 0 result |
| 4 | Issue RGBA 2; return D 1 | Idle | Oracle lane 1 |
| 5 | Issue D 2; return RGBA 2 | Idle | Hold results |
| 6 | Issue RGBA 3; return D 2 | Idle | Oracle lane 2 |
| 7 | Issue D 3; return RGBA 3 | Idle | Hold results |
| 8 | Return D 3 | Idle | Oracle lane 3 |
| 9 | Idle | Write passing color lanes | All four lane results retained |
| 10 | Idle | Write enabled passing depth lanes | Color commit precedes depth |
| 11 | Idle | Idle | Publish ROP completion and release slot |

Thus the measured uninterrupted hot interval is **13 cycles/quad**, not the target
8, even before replacing assumed oracle latency with certified arithmetic.
REPLACE and depth-disabled cases use the same conservative calendar. Sparse masks
skip framebuffer lane accesses but retain eight output rows and the same interval.
Same-address work waits for the previous quad's phase-10 depth write and retirement.
The model reports both total serialization wait and the subset with overlapping
covered coordinates. Maintenance and ROP do not overlap in this baseline; there
is no hidden claim that the available second port is already exploited for hits
under miss. The four-operation cache-port lower bound is not overall latency.

The serial baseline's 8-cycle target gap is explicit:

| Current contribution | Enabled cycles | Possible overlap, not implemented |
| --- | ---: | --- |
| Tag lookup/start | 1 | Predecode the next published/header-stable slot; recheck tag ownership at issue |
| Eight output row issues | 8 | Irreducible 1R bandwidth; next quad can begin on the next cycle with separate retained context |
| Final row return | 1 | Can coincide with the next quad's first read, after transferring pending-return ownership |
| Color then depth writes | 2 | Can coexist with next quad output reads, but consume distinct A-port phases |
| Completion/release | 1 | Can run beside the next quad; requires separate output-read and ROP retirement ownership |

The selected bounded pipeline below overlaps these contributions and keeps the
serial constructor as a correctness/control baseline.

## Fixed II8 control pipeline

`Model::new_pipelined` uses the same two output slots and five BSRAM candidates.
An independent hit quad starts every eight enabled cycles. Relative to admission,
output reads issue at 0..7 and return at 1..8; A depth/color reads issue at 0/1;
lane arithmetic issues at 2/4/6/8 and returns at 4/6/8/10; A color/depth writes
occur at 11/12; commit is at 13. Thus A modulo phases 0/1 are reads and 3/4 are
writes. A phase trace checks every bank's single operation. B maintenance remains
serialized with the entire ROP pipeline and starts only after both descriptors
and every return token have drained.

The arithmetic return delay is explicitly **two enabled cycles**, held across
CE pauses. These are oracle-result fixture registers, not an implementation of
blend/round/quantize in two DSP cycles. With ordinary retained-register consumption,
the chosen fixed calendar allows at most two arithmetic cycles: lane 3 returns
at 10 for color consumption at 11. A longer real datapath requires a new calendar
and retained-value proof; no RTL timing or physical-II claim is made here.

At offset 8, the last row returns and `output_payload_captured` releases the slot,
while a compact descriptor retains header, mask, line and age until commit. The
next producer starts filling that slot on the following cycle. At steady-state
offset 16 its eighth row can publish on the same edge as ROP reads its first row:
the first seven rows are already stored and row 7 and row 0 are different addresses.
This local publish/admit bypass and header/tag predecode are explicit combinational
control assumptions requiring later timing validation. Partial records otherwise
remain invisible. Both publication and consumption require CE. Exact CE cuts at
capture, the publish/read bypass, and shared-result replacement are regression-tested.

Register lifetime reuse replaces duplicated contexts:

| Shared state | Lifetime and ownership |
| --- | --- |
| Four old color/depth values, 128 bits | Lane 3 consumes the old values at offset 8 before the next quad replaces depth on that edge; color replacement follows at 9 |
| One RGBA, 32 bits | Row return of each even row retains RGBA for the next odd row only |
| Four results plus write flags, 136 bits | Old color consumed at 11, old depth at 12; next quad lane 0 returns at 12 **after** that old-depth read, lanes 1/2/3 replace their entries at 14/16/18 |
| Two compact descriptors | Each owns header21, line3, output-slot1, age4 and valid1 = 30 bits; mask stays in header and per-lane depth/color write enables travel with results |
| Row-return register | Data32, row3, descriptor1, valid1 = 37 bits |
| Two arithmetic token registers | Each result34, lane2, descriptor1, valid1 = 38 bits; explicit return owner assertions |

Together with phase3, the pipeline state is 60+37+76+32+128+136+3 = **572 logical
bits**, replacing the serial ROP's 552 plus active-valid bit: a **19-bit increase**
in retained logical state. The model modes are alternative implementations; they
do not instantiate both working sets in hardware. The common state below is
unchanged, giving 67016 logical modeled bits for this variant. Arithmetic internal
registers, tag/RAW comparators, muxes and control cones remain unmeasured, so 19
bits is not a fitted FF/Logic delta. No second 552-bit working set is allocated.

A new header overlapping any uncommitted descriptor cannot issue cache reads.
The next fixed admission opportunity after commit is offset 16, so repeated
same-address quads have II16. This is an explicit conservative RAW stall. The
descriptor retains identity even after output-slot reuse; faults discard pending
compute work while the same shared maintenance FSM drains accepted transactions.

The 80-quad uninterrupted independent-hot regression commits strictly every eight
cycles. It checks all final bytes against the independent golden; CE, depth/blend
modes, masked updates, replacement and terminal faults also run through this path.
The same 48-quad fixture probe gives 735 cycles for independent or sparse work,
1111 for same-address work (235 RAW-wait cycles), and 16910 for dirty rotation.
Traffic bytes remain identical to the serial table. Input stalls are respectively
169, 169, 529 and 14473; peak output occupancy remains two. These include demand
refill/final flush and are not MC measurements. Reproduce with the probe command
below followed by `--pipelined`, using a separate `target/framebuffer-pipeline`
output directory.

## Lane forwarding profile

`Model::new_forwarding` adds issue-time forwarding to the fixed II8 calendar.
`new_pipelined` retains the original RAW-stall path for comparison. Admission
captures the intersection of the new coverage mask and the sole older live
descriptor's coverage when their canonical even x/y coordinates match. This
dependency mask and one-bit owner are retained in the consumer descriptor;
forwarding never needs to look up a retired producer descriptor. Each lane issue
selects the producer's **final color and depth**, including preserved old values
after depth rejection, alpha zero, or disabled depth writes. Nondependent lanes
use their synchronous cache capture. A different quad in the same tile or a
quad in another line cannot alias this coordinate comparison.

The existing 136-bit result register bank is the only forwarding data source.
Each lane entry now carries owner1+valid1, updated only when a covered lane's result
returns; uncovered lanes leave the entry untouched. A covered depth-failed lane
still publishes its preserved pixel. Descriptor allocation/reuse does not clear these bits. Conceptual
`results_at_edge`/`owners_at_edge` values in the Rust model are combinational
aliases of the FF outputs, not extra retained arrays: same-edge returns become
visible only on the following edge. Cache writes also sample pre-edge FF outputs.

For producer A admitted at 0 and consumer B admitted at 8:

| Lane | A result register written | B reads A at issue | B overwrites same result entry |
| --- | ---: | ---: | ---: |
| 0 | 4 | 10 | 12 |
| 1 | 6 | 12 | 14 |
| 2 | 8 | 14 | 16 |
| 3 | 10 | 16 | 18 |

At 12, all old A depth values are sampled for the cache before B lane 0 updates
its result FF. B lane 1 reads A lane 1 on that edge, so its operand does not depend
on B lane 0's write. At 16, B lane 3 reads A lane 3 while B lane 2 updates its own
entry and C reads cache depth. C may reuse A's descriptor index then; allocation
does not touch result ownership. C's lane-3 return cannot overwrite that entry
until 26, well after B consumed it at 16. This fixed lifetime excludes an owner
ABA ambiguity without a wider generation tag. If B does not cover a lane, C
reads that lane from cache, where A has already committed at 13. Thus alternating
masks do not need a second-previous-quad forwarding table.

The forwarding readiness condition by itself is producer return latency <=7
enabled cycles (return must precede a consumer issue eight cycles later). The
selected cache-write calendar still limits the **whole implementation to the
existing assumed two-cycle arithmetic return**. These bounds do not establish
that blend/round/quantize fits a real two-cycle DSP/logic implementation.

A missing or wrong-owner entry never falls back to a stale cache operand. The
consumer has not reached its cache-write phases when checking at ages 2/4/6/8.
The controller keeps its complete output slot and header, cancels only that
consumer's arithmetic tokens/descriptor, and sets a restart flag. All older
returns, writes and retirement continue. Once they drain, the consumer restarts
from the now-committed cache; no global CE freeze is used for this dependency.
At age 8 the check precedes payload release and new admission. Earlier returned
consumer lanes have not written cache; they cannot corrupt older writes: lane 0
overwrites A only on the edge that A depth was already sampled, and later lanes
return after A's final cache write. Tests inject not-valid and wrong-owner cases
at each of the four ages and compare the full image with the stall baseline.

Complete incremental storage/control cost relative to the unforwarded profile:

| Addition | Width / implementation requirement |
| --- | --- |
| Two retained dependency records | 2x(mask4 + owner1) = 10 bits; zero mask encodes no dependency |
| Four result owner/valid entries | 4x2 = 8 bits; no new color/depth payload |
| Restart-pending flag | 1 bit; blocks speculative readmission until older descriptors drain |
| Immutable profile enable | 1 bit if retained as configuration; removable when tied on at elaboration |
| Coordinate match | Up to two 17-bit equality comparisons against valid descriptors, validity gating and owner selection; only one older descriptor can be live at admission |
| Coverage dependency | Four mask ANDs after selecting the matching producer |
| Issue operand select | One shared 4-to-1 32-bit result read mux and a 32-bit cache/forward 2-to-1 mux for the active lane; owner/valid 4-to-1 selection, one-bit owner equality and dependency test |
| Restart control | Cancel checks for two token owners, descriptor/slot retention, admission gating and retirement/drain checks |

This is **20 additional logical bits** including the immutable enable (19 if tied
on), 592 bits for the selected ROP pipeline/control, and **67036 total modeled
logical bits**. Comparator/mux/control logic is explicitly additional, not charged
as zero area; there is no fitted LUT/FF or timing estimate. No full-quad copy or
deeper output FIFO is introduced. Real arithmetic internal registers remain outside
this control experiment, as in the unforwarded profile.

The 96-quad same-address long-chain regression commits every eight enabled cycles,
forwards 380 lanes and takes no restart. The 80-quad real-cycle MC same-address
test also maintains II8 and matches the independent full-image golden. Coverage
includes all depth/blend/depth-write combinations, alternating masks, alpha zero,
depth rejection, exact CE cuts at shared-result replacement and descriptor reuse,
producer backpressure, different coordinates/lines, and fault/drain. The original
real-MC dirty-rotation test remains unchanged.

In the finite 48-quad fixture probe, same-address work now takes 735 wall cycles
instead of the unforwarded 1111, with 188 forwarded lanes, zero RAW waiting and
zero restart. Independent/sparse and dirty-rotation totals remain unchanged.
Use `--forwarding` instead of `--pipelined` and a separate
`target/framebuffer-forwarding` output directory. These are model scheduling
results under the existing two-cycle arithmetic assumption, not RTL/PnR evidence.

## Complete modeled storage and ports

The serial-baseline table counts logical state at the declared signal width/domain, excluding
Rust padding and diagnostics. It includes retained old/result copies. Domain-sized
indices are explicit implementation candidates, not a synthesized area claim.
Tags retain 16 bits even though the maximum configured surface uses fewer. Optional
values include a valid bit. Physical arithmetic pipeline registers remain unmeasured.

| State | Bits | Ports/owner |
| --- | ---: | --- |
| Four 1024x16 cache banks | 65536 | One R/W A per bank for ROP; one R/W B per bank for maintenance |
| Two 8x32 output payloads | 512 | One final W plus one synchronous ROP R; physical candidate one 512x36 BSRAM |
| Eight tags + valid + two dirty bits | 152 | Single control owner; no parallel replacement while pinned |
| Maintenance: line3, target16+valid, write1, plane1, sector2, accepted1, presented1, beat5, skid64+valid | 96 | One active transaction; one-beat capture credit |
| ROP: line3, phase4, header21, row-return32, source RGBA128, old color/depth128, result color/depth128+write flags8 | 552 | One active context; four retained lanes |
| Output headers42 + present2 + ready2 + head1 + tail1 + fill-row4 | 52 | Two total slots, including partial producer slot |
| Surface: two bases64, width10, height9 | 83 | Immutable configuration |
| Context: depth-func3 + depth-write1 + blend1 | 5 | Immutable configuration |
| Victim3, flush1, flush-complete1, fault1, miss-wait1, active-ROP1, active-maintenance1 | 9 | Control owner |
| **Total logical modeled state** | **66997** | Excludes trace/statistics and external fixture memory |

The four cache BSRAMs reserve four physical blocks; unused parity is not free RAM.
The output block reserves one further whole BSRAM (18432 physical bits) despite
only 512 payload bits used. No extra tile/128-byte staging array exists. The write
skid is the only maintenance data buffer. The independent host image, replay
stimulus slice, returned trace vector, statistics, and CSV formatting are host
diagnostics, never on-chip queues. This is not a complete Logic/FF/DSP fit; arithmetic
registers and combinational address/selection cones still require counted/RTL work.

## Finite validation and reproduction

The bounded tests compare the entire byte image, including untouched guard regions,
against a separate real-number golden with independently derived addresses and
quantization. They exercise partial coverage, repeated quantization, every depth
function, depth-write on/off, both blends, all 65536 RGB565 round trips, all bank
addresses, dirty eviction, flush, request/beat backpressure, CE, terminal errors,
external fault drain, delayed ACK, and unpublished output protection. No unlimited
simulation loop is used.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label framebuffer -CargoArgs @('-p','gpu-v2','--test','framebuffer')
& scripts/run-cargo.ps1 -Subcommand run -Label framebuffer-probe -CargoArgs @('-p','gpu-v2','--example','framebuffer_probe','--','target/framebuffer')
```

The probe emits deterministic per-cycle A/B calendars and a summary for 48 quads
each of independent hot-tile coordinates, repeated same-address coverage, sparse
mask1, and 20-tile dirty rotation. It includes the initial demand refill and final
flush. Default fixture request/beat periods are one cycle, with ACK delayed three
cycles. The output source continuously offers rows subject to backpressure.

| Fixture workload | Wall cycles | Hits / misses | Read / write bytes | Bytes / covered pixel | Min commit interval |
| --- | ---: | --- | --- | ---: | ---: |
| Independent quad | 962 | 47 / 1 | 1024 / 1024 | 10.667 | 13 |
| Same address | 962 | 47 / 1 | 1024 / 1024 | 10.667 | 13 |
| Sparse mask1 | 962 | 47 / 1 | 1024 / 1024 | 42.667 | 13 |
| Dirty rotation | 16432 | 0 / 48 | 49152 / 49152 | 512.000 | 174 |

Same-address serialization contributes 288 waiting cycles; independent coordinates
have zero RAW waiting. Every scenario reaches the two-slot occupancy limit. Full
input/maintenance/serialization stalls and each access are in the generated CSVs;
counters describe overlapping conditions and must not be summed as a partition
of wall cycles. Dirty rotation's minimum interval includes cold maintenance and
is not a hot-cache II. These finite fixture results make no complete-system
throughput, MC bandwidth, audited arithmetic, PnR, or physical-board claim.

## Shared transport and real-cycle MC comparison

The test-only [burst adapter](../tests/support/sdram/burst.rs) projects the existing
vendor `Combination` at each wall-clock edge. It does not queue whole write
payloads or add latency constants. Framebuffer library code only imports GPU-owned
ports. Writeback presents the prefetched skid word together with its request,
then advances only on `write_accepted`; B capture fills the next skid word on that
edge. This meets the MC continuous-source requirement even with compute CE low.
Read LAST and successful completion may occur on the same edge.

`real_cycle_mc_refill_dirty_rotation_and_flush_match_golden` runs 24 quads over
20 tiles, partial/full coverage, and a compute pause every seventh wall cycle.
It compares the entire returned image and guards with the independent golden,
checks every write ACK follows the final data beat, and confirms serial operation
with zero read/write chains. The bounded run completes in **10254 wall cycles**,
with 80 covered pixels, 24 misses, 24576 bytes read, 24576 bytes written, and 192
write ACKs. The per-transaction trace and counters are regenerated under
`target/framebuffer-real-mc/`. These are this independent MC model's finite
results, not a loaded complete-system or physical-board measurement.

Additional protocol tests terminate a read after three beats with `complete(false)`
and fault a blocked request before acceptance. They verify early errors drain
without publishing a tag, and presented valid remains stable through the terminal
event. The public adapter's own request/data stability, alignment, source underrun
and last-beat/ACK checks run alongside the framebuffer suite:

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label framebuffer-memory -CargoArgs @('-p','gpu-v2','--test','framebuffer','--test','memory_burst')
```
