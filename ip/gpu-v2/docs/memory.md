# Standalone memory interface

## Masters and address units

The descriptor-based GPU exposes three main-system masters:

| Master | Traffic | Direction |
| --- | --- | --- |
| RO | Geometry/config loader and texture refill | Read-only, transaction-level shared owner |
| Framebuffer read | Color/depth tile refill | Read-only |
| Framebuffer write | Dirty color/depth sectors | Write-only |

Addresses are 22-bit halfword units, covering 8 MiB. Data beats are 64 bits.
Count codes 0/1/3 select 32/64/128 naturally aligned bytes; code 2 is invalid.
A 128-byte sector carries 16 beats. The loader selects sizes from remaining
length and natural boundaries; a tail shorter than 32 bytes still reads 32 bytes
and copies only its valid qwords into local storage.

Texture and framebuffer internally use 32-bit byte sector addresses. Adapters
check alignment/range, then divide by 2 for the main-system address. Framebuffer
writes stream from the cache without a second whole-burst payload FIFO.

## Request, data and terminal ownership

A presented request holds valid, address and count until request-ready accepts
it. This transfer reserves the master and return destination. Responses have
no ready input: the destination must already have room for the whole admitted
read. Each accepted response belongs to its ordered owner through actual
last/error, regardless of compute CE.

At the actual main-system pins, distinguish:

| Event | Pin condition |
| --- | --- |
| Read data beat | `ro_response_valid` or `fb_r_response_valid`, with no response error |
| Successful read terminal | The same valid beat also asserts `*_response_last`; its index is the expected final beat |
| Write data beat | Accepted `fb_w_data_ready` while the cache offers its held data |
| Successful write terminal | `fb_w_response_valid && fb_w_response_last && !fb_w_response_error`, after all 16 write beats |
| Error terminal | A response-valid edge with response-error; it can terminate early |

Read last and read success terminal share an edge here. Write last data and write
terminal response are different events; the response may coincide with the last
accepted data beat. A return context becomes active after request acceptance, so these
physical pins must not return a response on that acceptance edge. Separate
byte-facade test interfaces may have other explicitly documented edge rules.

Framebuffer write data holds through data-ready stalls. A write source retains
all remaining beats and must sustain the accepted burst's data contract; the
physical controller cannot accept arbitrary mid-burst valid holes. The final
write data beat does not itself acknowledge the transaction. Terminal response
ends the owner. Earlier writes survive an error.

RO arbitration is round-robin per transaction between geometry and texture.
The explicit command factory replaces the geometry loader with its unified
command/data DMA and uses two RO clients, DMA and texture. It does not enable
the optional third control client or add a physical memory master.
Two ordered RO return tags retain each accepted choice until its own last/error.
A held request is separate from the active return owner; responses never
interleave owners at beat granularity. Framebuffer keeps at most two accepted
sector descriptors, with one direction for the entire maintenance phase. Its
payload and terminal cursors advance independently: supplying the next sector
does not wait for the preceding write acknowledgement. The vendor arbiter
controls contention between all system clients.

For the command factory, the router also qualifies framing before the DMA RAM
write in its independent checked component API. The production command closure
specializes that diagnostic feedback out: passive simulation monitors check
framing and peer errors without gating writes or progress. Its legal peer
contract requires the complete declared burst and correctly placed LAST.
Command DMA keeps only a scratchpad write cursor; burst length/framing remains
in the router's return context, with no duplicate DMA return-length counter. See [command diagnostics](/ip/gpu-v2/docs/commands.md#diagnostics-cancellation-and-scope)
for the production behavior and explicit-abort boundary.

## Pause and clocks

Compute CE controls scheduled arithmetic and its owned synchronous returns.
It does not gate accepted memory requests, response handling, loader DMA writes,
completion/page publication or framebuffer maintenance. An offered request also
cannot disappear merely because compute pauses.

A composition advances the shared memory service exactly once per wall edge.
Several clients may submit/poll that service, but they do not each tick the
physical controller. Host service-queue acceptance and physical master acceptance
are distinct events; [reference memory adapters](/ip/gpu-v2/docs/references/memory.md) define
their own interface.

## Errors and cancellation

The following error-response behavior describes the checked standalone memory
API. The command factory uses the passive-monitor specialization described
above; explicit abort and accepted-transaction ownership remain functional.

Abort stops new computation and future loader work. A request already presented
remains stable until acceptance, then drains to terminal. Router faults cancel
future loader work; only the locked owner receives the current response.
Malformed responses fault instead of silently changing ownership.

No transactional rollback is provided for earlier local DMA or framebuffer
writes. Reset must be coordinated across the GPU, arbiter, bridge and physical
controller, after external ownership is safely handled. A local reset alone
cannot free an externally accepted transaction.

## Shared-system boundary

The Gowin vendor crate owns arbiter, adapter, bridge, controller and pin model.
The CPU board configuration enables one reserved successor request and
other-bank preparation; it does not make the framebuffer's four sectors an
indivisible group. CPU/GPU/Display contention must use that actual configuration.
A standalone transfer or leaf timing result does not establish board integration.

Implementation anchors: [router and adapters](/ip/gpu-v2/src/system/vliw_memory.v),
[three-master wrapper](/ip/gpu-v2/src/system/vliw_sdram.v),
[vendor interface](/hardware/vendor/gowin/doc/README.md).

## Independent native continuation

The validation combination exported by
`combination::rtl_sources(init)` uses the vendor's
streaming endpoint and related-clock bridge. Same-owner, nonblocking GPU RO
reads, framebuffer refill reads and framebuffer writes can continue native commands when bank/row,
refresh age, direction and write supply allow it. Each32/64/128-byte transaction
still has an independent LAST/response. Other owners and CPU response stalls
fall back to ordinary ordered completion. A 512-byte facade job is segmented into independent hardware transactions.

The ordinary framebuffer cache uses three narrow cursors in a single locked
16x8-pixel maintenance context: request segment, payload qword and retired
segment. Segment bits are `{plane,sector}`. Each plane is256 bytes and uses two
128-byte transactions. A full refill uses four; dirty writeback uses two or four.
The request valid is registered and held until acceptance. It replaces the old
separate presented-state control: prefetch cancellation cannot form a
combinational path through the arbiter into another client's ready. The first
offer may take one preparation edge; accepted consecutive sectors remain
continuous. Addresses advance only on acceptance.

The router holds one direction for a maintenance phase and a two-bit credit
count, rather than indexed direction records and enqueue/dequeue cursors. The
four BSRAM B outputs hold the sole write beat; consuming that beat launches the
next synchronous read, including across sector boundaries. Ordered bounded
payload cursors use equality and final-word decoding, with monotonicity checked
by passive assertions. Refill returns write directly to the banks. Request
presentation, payload and retirement continue when compute CE is low. No
payload FIFO or additional BSRAM is introduced.

Dirty writeback must fully retire before the old tag is invalidated and refill
starts. The new tag is published only after all four refill sectors retire.
Flush uses the same writeback path, then invalidates coordinate tags. Prefetch
abort discards queued hints, cancels only sectors never presented, and drains
held/accepted sectors without publishing a partial line. Read/write turnaround,
refresh, competing clients or a late/unprepared successor can still introduce
gaps. The native controller prepares an eligible other bank when its descriptor
arrives, rather than sampling it only at the first word.

The streaming bridge publishes each stable next-address window with a one-bit
generation. The controller registers the row-hit result and its generation
together; chain qualification checks that bit rather than repeating a wide
bank/row identity comparison on the final acceptance path. A new hint or an
unhinted actual offer changes the generation. Promoting the ordered hint to its
actual continuation preserves it. The endpoint's full two-descriptor queue
keeps a published hint immutable, so no producer-side address comparator is
needed. Passive simulation assertions check both stability and promotion.
An unknown generation waits for the lookup; it is not treated as a row miss.
Speculative preparation also consumes that registered result and waits through
the ACT/RCD update. Request ownership and real terminal responses are unchanged.

CLEAR supplies its constant directly. Consecutive CP DMA requests and meshlet
segments use the same path, without a separate filler or payload storage.
The complete CPU board uses the same streaming endpoint/bridge and three
system-owned descriptor registers before arbitration; see
[system admission](/systems/cpu-v3-tang-nano-20k/docs/architecture.md#streaming-memory-admission).
UI uses four-sector color maintenance and grouped atlas reads as described in
[UI](/ip/gpu-v2/docs/ui.md#storage-and-pipeline). The fitted vendor factory exposes only this stream closure. Standalone pin
simulation and isolated timing do not qualify a changed complete board image.

The bounded `tests/framebuffer_continuation.py` fixture exercises the actual
production cache/router modules from `--gpu --release`, native DPB/shared DSP,
shared arbiter/controller and SDRAM pin model. It checks varying write data,
the complete color/depth footprint and guards, independent terminal counts,
compute pause, color-only dirtiness and refresh fallback. `--expect-chain`
checks both the native acceptance events and32-native-clock column spacing;
CLEAR/RO tests alone do not establish framebuffer continuation.

Bounded reproduction: export the actual peer with the `vliw_sdram` test, then
run `tests/command_continuation.py --export <command.v> --peer <mc-peer>
--output <new-directory>`; add `--clear` for both complete planes. For a short
qualification use `--dma-jobs 8 --meshlet-blocks 2 --nops 24`, which still crosses
a command page, or `--clear --clear-stop-after 8` to check a short accepted prefix
and explicit-abort drain. The test
checks exact scratch writes, command-page crossing, independent completion,
full pin-memory clear contents and guard words. It preserves DUT/peer hashes.
`tests/framebuffer_router_stream.py` also checks simultaneous last write data
and terminal response, delayed acknowledgement, two credits and read/write phase
drain. Vendor `tests/stream_compatibility.py` covers scalar masks, held CPU responses
and GPU continuation after a held CPU LAST. All processes and simulations
have explicit bounds.

`hardware/vendor/gowin/tests/fit_continuation.py` compares preserved baseline
and candidate MC peers. Optional `--gpu-base` and `--gpu-candidate` connect the
actual three-master GPU to that same shared arbiter/MC. All output bits first
enter registers in their owning clock domain; only those registered values
feed observation reductions. The fixed probe is identical across variants.
These fits exclude CPU/display engines and physical SDRAM IO timing, and must
not be described as complete system or board qualification.

For a controlled mapping comparison, add `--fixed-gpu` with identical GPU input
files. The helper synthesizes one GPU netlist without port buffers and imports
that exact `.vg` into both connected trials. The RTL GPU is not recompiled by
either trial. Netlist hashes, the identical registered probe and post-PnR
simulation models are retained with the receipts.
`hardware/vendor/gowin/tests/compare_netlist_region.py` compares those models'
GPU cells, parameters and connectivity. It normalizes net names by incident
cell pins, including the fixed boundary registers, constants and clock source;
vector grouping is expanded to individual bits. `--flat-instance-names` also
normalizes the slash-to-underscore aliases used when importing a flat model.
These normalizations do not permit changed constants, truth tables or bit
ordering. An alias rename alone does not count as a circuit change. Resource and timing
deltas belong to their matched pair: separately compiling the GPU can change
the baseline mapping, so its absolute area is not interchangeable with the
ordinary monolithic build. This option is a diagnostic build flow, not a change
to the board target or its delivery qualification.

The streaming endpoint retains an accepted physical owner until a real read
LAST or write-done response retires it. A simulation-only timeout reports a
missing terminal; it does not manufacture an error/LAST response or reclaim a
live descriptor. Driver-owned malformed requests can receive an immediate
error only when no physical transaction was issued for that descriptor.
