# CPU V3 cache, fetch, and DRAM roadmap

Status: active handoff plan for future conversations
Repository: `D:\github\DigitalDesign-code`
Updated: 2026-08-30

## Milestone workflow and document lifecycle

- Keep this roadmap in the repository while any milestone remains incomplete.
- Before and after each milestone, update the current implementation state here so another agent can
  resume from the documented repository state.
- Include the updated roadmap in the same commit that completes each milestone.
- Delete this roadmap only in the final milestone commit, after every milestone has been completed
  and accepted.
- Scale validation to the risk of the change. Correctness-focused changes with little timing or
  resource pressure may use a smaller targeted test set, provided the completed milestone records
  what ran and what was intentionally skipped. Changes that affect timing, resource use, memory
  geometry, clocks, or clock-domain crossings still require the corresponding hardware validation.

## Current implementation progress

| Stage | State | Result | Boot / SDRAM PnR at 54 MHz |
| --- | --- | --- | --- |
| 0 | Complete, 2026-08-29 | Removed per-line snoop/invalidate; froze global maintenance and boot-handoff semantics. | 4 BSRAM; 55.435 / 55.958 MHz |
| 1 | Complete, 2026-08-29 | Added private 256-bit refill buffers and complete-line commit. | 4 BSRAM; 57.127 / 62.440 MHz |
| 2 | Complete, 2026-08-29 | Replaced serialized reads with one real `8 x 32-bit` SDRAM burst. | 4 BSRAM; 55.327 / 56.090 MHz |
| 3 | Complete, 2026-08-29 | Split each cache into even/odd BSRAM banks; initialization contents are split by word parity and refill drain is eight cycles. | 6 BSRAM; 54.492 / 54.261 MHz |
| 4 | Complete, 2026-08-29 | Converted both caches to two ways with invalid-way-first deterministic victim replacement. The tag comparison now precedes the data-bank read, so a hit costs one more registered cycle until Stage 5 pipelines it. | 6 BSRAM; 61.425 / 56.530 MHz |
| System consolidation | Complete, 2026-08-30 | Folded the separate CPU V3 boot, SDRAM, and display systems into one fitted `cpu_v3_system`. This changed the full-system baseline to 7 BSRAM before the Stage 5 fetch pipeline work. | 7 BSRAM; 57.345 MHz |
| 5 | Complete, 2026-08-30 | Pipelined resident cache reads for one accepted lookup per cycle and added a four-entry, epoch-tagged instruction fetch queue. Sequential ALU throughput now approaches two cycles per instruction. | 7 BSRAM; 61.842 MHz |
| 6 | Complete, 2026-08-30 | Added demand-progress-triggered, low-priority next-line I-cache prefetch with redirect cancellation, discardable in-flight refills, simulation counters, and demand-safe cancellation races. | 7 BSRAM; 54.538 MHz |
| 7 | Complete, 2026-08-30 | Added cycle-profiled emulator benchmarks and a redirect fast path. Immediate target issue plus empty-queue response fall-through reduced hot control-transfer fetch waits from four cycles to two. | 7 BSRAM; 57.293 MHz |
| 8-12 | Not started | See the ordered tasks and detailed stage sections below. | - |

## Ordered major tasks

0. Remove the unused per-line snoop/invalidate direction now, freeze the global maintenance ABI and software ownership contract, and put I-cache invalidate immediately before the final boot jump.
1. Add one private 256-bit refill buffer between each cache and DRAM.
2. Change cache-line refill, the arbiter, and the SDRAM adapter to one real `8 x 32-bit` burst.
3. Change each I-cache and D-cache to two BSRAM blocks split by even and odd words, doubling internal bandwidth.
4. Change each I-cache and D-cache to two ways, including two tag lookups and a deterministic replacement policy.
5. Pipeline the I-cache hit path and add a small instruction fetch queue in front of the CPU.
6. Add low-priority next-line I-cache prefetch, reusing the 256-bit refill path.
7. Profile representative workloads and shorten control-flow redirect recovery without branch prediction.
8. Change D-cache to write-back and implement dirty eviction plus global clean/invalidate maintenance in the same change.
9. Integrate the already-frozen software ownership contract into DMA, and display APIs without changing the cache core again.
10. Run DRAM at twice the CPU frequency through an explicit asynchronous FIFO or equivalent CDC boundary.
11. (After GPU implementation) Complete GPU integration and final memory arbitration.
12. Close full-system timing at no less than CPU 50 MHz and DRAM 100 MHz, then run concurrent hardware stress tests.

Task 0 should happen before Tasks 1 and 2. Tasks 1 and 2 remain the next performance implementation
scope after that policy cleanup. Do not combine all tasks into one change.

## Architectural decisions

- A cache line remains public and fixed at 16 physical 16-bit words, or 32 bytes.
- One DRAM line transfer is eight ordered 32-bit beats.
- I-cache and D-cache each own a private eight-entry 32-bit refill buffer.
- The first refill revision keeps the current direct-mapped, single-BSRAM cache geometry.
- The refill buffer is the future CPU/DRAM clock-domain boundary, but its first synchronous implementation is not itself a CDC solution.
- Two-way associativity, two-BSRAM banking, fetch pipelining, next-line prefetch, and write-back are separate milestones.

## Stage 0: remove dead coherence machinery and freeze the final contract

Do this while D-cache is still write-through. It is a small deletion and contract change, not an
early implementation of write-back maintenance.

Current audited state at branch `code-v0.3-dev`, commit `5775bf3`:

- The architecture and RTL caches are direct-mapped, write-through, and no-write-allocate.
- There are no dirty bits, write-back states, clean states, or maintenance busy/completion states.
- `snoop_write_valid` and `snoop_write_address` are not driven by any production system; every system
  cache instance ties them to zero. Only the cache model/RTL and a local test exercise them.
- The Rust transaction model still exposes `invalidate_line`, but no production ownership path uses it.
- Device 0 channels 0 and 1 already perform whole-I-cache and whole-D-cache invalidation.
- Stage0 and Stage1 already use those whole-cache operations at their final handoff.
- `SharedBufferOwner` already models software ownership and requires accelerator DRAM writes to finish
  before CPU reacquisition; it does not implement hardware coherence.

Stage 0 changes:

- Remove `snoop_write_valid` and `snoop_write_address` from the cache Rust interface, generated RTL,
  Verilog template, all tied-zero instances, and tests.
- Remove `invalidate_line` and its DMA/per-line test from the architecture model.
- Keep only full-cache invalidation in the current write-through cache.
- Reserve semantic ABI names for future `ICACHE_INVALIDATE_ALL_DELAYED`, `D_CLEAN_ALL`,
  `D_INVALIDATE_ALL`, and final success/error status. Do not implement a fake clean scanner before
  dirty state exists.
- Define `D_INVALIDATE_ALL` as a cheap valid-bit clear while D-cache is write-through. When Stage 8
  introduces dirty state, the same architectural operation becomes clean-plus-invalidate and holds
  the CPU until all dirty writes finish.
- Expose semantic cache APIs rather than raw public maintenance-channel sends. The compiler/runtime
  boundary must treat them as memory and control barriers.
- Rename ownership predicates from vague store completion to DRAM visibility or clean completion so
  their meaning remains correct after write-back.

Add a non-returning CPU V3 intrinsic with semantics equivalent to:

```text
icache_invalidate_delayed_and_jump(cseg, target) -> !
```

Represent it as one terminal compiler IR operation. Lower it to two adjacent machine words in this
exact order:

```text
DEVSEND ICACHE_INVALIDATE_ALL_DELAYED
JSEG cseg, target
```

It cannot lower as `JSEG; DEVSEND`, because CPU V3 has no jump delay slot and the second word would
never execute. The semantic operation is atomic from software's point of view even though the encoded
invalidate command precedes the jump. No optimizer, register-allocation spill, helper call, or ordinary
instruction may be inserted between the two words.

Change the canonical boot tail now to this shape:

```text
dcache_invalidate_all()      currently one-cycle because write-through
prepare final DSEG/registers
icache_invalidate_delayed_and_jump(cseg, target) -> !
```

After Stage 8, the helper waits for `D_INVALIDATE_ALL` completion before preparing the final redirect;
the intrinsic's final `ICACHE_INVALIDATE_ALL_DELAYED; JSEG` adjacency remains unchanged.

Do not hard-code system-control device/channel decoding into the generic CPU execute state merely to
create the delay. Keep the system-control invalidate output registered and feed that architectural
pulse to both I-cache and the instruction fetch frontend. When fetch pipelining is added, the frontend
uses the pulse to advance its epoch and discard queued or outstanding old-path words. This preserves
the existing generic device bus while giving the intrinsic deterministic behavior.

Stage 0 acceptance:

- A repository search finds no cache snoop ports, per-line invalidation API, or tied-zero snoop wiring.
- Both full-cache invalidations still pass model and RTL tests.
- Stage0 and Stage1 boot tests prove the canonical I-cache-invalidate/final-jump adjacency.
- Existing DMA boot and display tests remain valid under the explicit ownership model.
- BSRAM use and timing do not regress.

## Stage 1: private 256-bit refill buffers

Each cache adds storage equivalent to:

```text
refill_buffer[0:7] : 32 bits per entry
refill_beat        : 0..7
drain_word         : 0..15
refill_error       : one sticky bit
```

Target miss flow:

```text
CPU miss
  -> cache issues one aligned line request
  -> DRAM returns eight ordered 32-bit beats
  -> cache captures all beats in its private 256-bit buffer
  -> arbiter releases the DRAM owner after the physical last beat
  -> cache privately drains sixteen 16-bit words into its existing BSRAM
  -> tag and valid state commit only after a complete error-free line
  -> original CPU request receives its response
```

The low half of beat `n` is cache word `2*n`; the high half is word `2*n+1`.
An error or invalidate must never expose a partially installed line.

Suggested cache states:

```text
IDLE
CHECK
WORD_REQUEST
WORD_RESPONSE
LINE_REQUEST
LINE_RECEIVE
LINE_DRAIN
CPU_RESPONSE
```

Writes remain word transactions during this stage. Read misses use line transactions.

## Stage 2: real SDRAM burst refill

- Replace sixteen independent 16-bit read commands with one aligned cache-line command.
- Return exactly eight 32-bit response beats with explicit `valid`, `ready`, `last`, and `error` semantics.
- Hold arbiter ownership through the physical last-beat handshake, not through the private BSRAM drain.
- Define behavior under refresh and backpressure; beats must remain ordered and stable while stalled.
- Preserve display/GPU deadline traffic and prevent speculative traffic from blocking demand requests indefinitely.

Acceptance:

- One cold read miss produces exactly one line command and exactly eight accepted beats.
- No cache issues sixteen word-read commands for a line refill.
- A second client may acquire the arbiter after beat seven even while the first cache drains its buffer.
- Error injection leaves the destination cache line invalid.
- I-cache and D-cache can independently hold one completed or in-progress refill.

## Stage 3: two parity-split BSRAM blocks per cache

Use two physical BSRAM blocks per cache:

```text
even bank: words 0, 2, 4, ... 14
odd bank:  words 1, 3, 5, ... 15
```

For the later two-way geometry, each bank address is conceptually:

```text
{ way, set[5:0], pair_index[2:0] }
```

This is exactly `2 ways x 64 sets x 8 entries = 1024` addresses per bank. A 32-bit refill beat can write its even and odd halves in the same cycle, reducing the private drain from sixteen cycles to eight.

Do not claim that two 50 MHz 16-bit banks directly absorb the full output of a 100 MHz 32-bit DRAM interface: they sink 32 bits per CPU cycle while DRAM can produce the equivalent of 64 bits per CPU cycle. The refill FIFO remains necessary.

## Stage 4: two-way caches

- Keep 64 sets and 16 words per line; capacity becomes 4 KiB per cache.
- Read both ways for the selected parity bank and compare both tags.
- Define one victim bit per set or another deterministic low-cost replacement rule.
- Use the victim bit as the next replacement way; after a successful refill it points to the other way. Hits do not update it.
- Prefer invalid ways before evicting a valid way.
- For D-cache write-back, dirty state is a later stage, not part of the first two-way conversion.
- Validate same-set alternating lines, invalid-way preference, replacement, invalidate, and refill failure.

## Stage 5: pipelined I-cache hit path and instruction fetch queue

This stage removes fixed hit latency. It is distinct from line prefetch.

Stage 4 hit timing for a simple ALU instruction was:

```text
cycle 1: FETCH_REQUEST
cycle 2: I-cache synchronous BSRAM lookup and tag check
cycle 3: registered cache response and instruction latch
cycle 4: EXECUTE and retire
```

That implementation retired one simple instruction per four cycles when every access hit. The
completed Stage 5 frontend removes the fixed per-word request/response bubble while retaining the
blocking execute machine.

Target organization:

- Make the I-cache hit interface pipelined: accept one lookup address per cycle when no miss, invalidate, or structural conflict prevents it.
- Return the corresponding word and hit/miss result with an explicit registered request tag or address.
- Add a two-to-four-entry instruction fetch queue between I-cache and the CPU core; start with four entries unless PnR shows a reason to reduce it.
- Let the fetch frontend request sequential physical word addresses while queue space exists.
- Let the core pop a queued instruction without performing the old request/response round trip for every word.
- Keep execution in order and allow at most one architectural instruction to retire per cycle; this task is not a full execute pipeline.
- A taken branch, `JALR`, code-segment change, fault, reset, or I-cache invalidate flushes all queued words and restarts fetch from the resolved physical PC.
- Attach an epoch/generation bit to outstanding fetch responses so a late response from a flushed path is discarded.
- On an I-cache miss, preserve the miss address, stop speculative issue as needed, refill the line, and resume without duplicating or skipping a word.
- Prefix words must retain exact architectural ordering with their consumer.

Initial performance target:

```text
simple sequential hit stream: no old per-word cache request/response bubble
conservative core target:      at most one queue-pop cycle plus one execute cycle
expected simple throughput:    approach one instruction per two cycles before a full execute pipeline
```

Do not set a one-instruction-per-cycle acceptance target until register dependencies, forwarding, branch resolution, and execute-stage pipelining are designed explicitly.

Acceptance:

- Sequential I-cache hits can be issued on consecutive cycles and returned in order.
- The fetch queue never overflows or underflows silently under response backpressure.
- Taken branches and code-segment changes execute no stale queued instruction.
- Invalidate during an outstanding lookup or refill cannot make a stale instruction architectural.
- Prefix/consumer tests, branches, boot handoffs, and fault PCs remain bit-exact.
- A cycle-count test demonstrates that a sequential simple-ALU loop improves from the current four-cycle baseline.
- PnR confirms that the pipelined tag/data/mux path meets the selected CPU clock.

## Stage 6: low-priority next-line prefetch

Only after the demand hit path and fetch queue are stable:

- Trigger a candidate next-line request from real CPU fetch progress near the end of a line.
- Never recursively trigger another prefetch merely because a prefetched line completed.
- Give demand I-cache misses, D-cache traffic, display deadlines, and GPU demand traffic priority over prefetch.
- Reuse the I-cache 256-bit refill path when it is idle.
- Cancel an unissued prefetch immediately; an unavoidable in-flight burst may complete, but its result must be discardable.
- Track `issued`, `useful`, `useless`, and `dropped` counters in simulation or debug builds.

Next-line prefetch reduces DRAM misses. It does not replace Stage 5 and does not by itself reduce I-cache BSRAM hit latency.

Stage 6 validation used emulator/RTL cycle-by-cycle co-simulation, the complete Icarus hardware suite,
the two-stage boot testbench, and a full Gowin rebuild plus current-artifact audit. The fitted system
used 7 BSRAM, 80 RAM16 leaves, 8,936 LUTs, 713 ALUs, and 3,656 logic flip-flops. It closed the 54 MHz
SDRAM/CPU clock at 54.538 MHz with zero setup and hold violations. On the checksum-protected 2,048-word
recursive quicksort, one prefetch was issued and useful, none was useless, and 20,208 candidates were
dropped because demand traffic or cache state had priority. The benchmark still took 2,652,077 cycles,
and its redirect trace exposed the separate control-flow recovery bottleneck addressed by Stage 7.

## Stage 7: profiled control-flow redirect fast path

- Add bounded full-system emulator benchmarks for recursive quicksort, control-flow-heavy code, and
  cached data traffic, with cycle categories, cache/SDRAM counters, opcode counts, and redirect waits.
- Export durable text summaries and per-redirect CSV traces under `target/cpu-v3-bench/`.
- Issue a redirect target request in the restart cycle when request metadata capacity is available.
- Tag that request with the new fetch epoch so old-path responses remain discardable.
- When the queue is empty, fall through a matching response directly to a ready core; if the core is
  backpressured, enqueue the response normally instead of dropping it.
- Preserve the existing bounded metadata FIFO behavior when an old-path response and redirect happen
  together. Do not add branch prediction or architectural delay slots in this stage.

Stage 7 validation used emulator/RTL cycle-by-cycle co-simulation, bounded Verilog regressions for
redirect issue, fall-through, backpressure, and stale epochs, the complete Icarus hardware suite, the
two-stage boot testbench, and a full Gowin rebuild plus current-artifact audit. The fitted system used
7 BSRAM, 80 RAM16 leaves, 8,927 LUTs, 747 ALUs, and 3,658 logic flip-flops. It closed the 54 MHz
SDRAM/CPU clock at 57.293 MHz with zero setup and hold violations.

The checksum-protected quicksort retired 708,531 words in 2,467,577 cycles (3.483 cycles per retired
word), saving 184,500 cycles, or 6.96%, from the Stage 6 baseline. Of 92,249 redirects, 92,244 hot
redirects waited exactly two cycles instead of four; the remaining five included cold I-cache misses.
The trace attributed 35.09% of all cycles to the data request/response path and 7.48% to fetch waits.
Its D-cache observed 152,482 loads, 44,457 write-through stores, and only 315 line refills, making
write-through store latency the next dominant optimization target. The full quicksort test runs in
release mode and is explicitly ignored by ordinary debug test runs.

## Stage 8: D-cache write-back and global maintenance engine

- Add dirty state per way and set.
- Evict a dirty victim before overwriting its data or tag.
- Complete write-back before installing the replacement line.
- Define error behavior so neither old nor new data is falsely reported valid.
- Keep device and uncached accesses outside normal cache allocation.
- Reuse the private 256-bit line buffer for dirty eviction and maintenance write-back where practical.
- A global clean engine scans all sets and ways, writes one dirty line at a time, and blocks new D-cache requests until it finishes.

Implement write-back and its complete global maintenance engine in this same stage. Expose only:

```text
ICACHE_INVALIDATE_ALL_DELAYED  one cycle later, invalidate every I-cache way and flush fetch
D_CLEAN_ALL                  write every dirty D-cache line, then leave lines valid and clean
D_INVALIDATE_ALL             write every dirty D-cache line, then invalidate every D-cache way
CACHE_MAINTENANCE_HOLD       internal CPU hold, asserted from command acceptance through completion
CACHE_MAINTENANCE_STATUS     final success or DRAM error
```

`D_INVALIDATE_ALL` is architecturally clean-plus-invalidate. Do not expose an unsafe command that
silently discards dirty data. D-cache clean and invalidate are variable-latency operations and are not
complete until every accepted DRAM write-back has completed successfully. Implementing these states
together with dirty eviction prevents a temporary range/snoop protection design from being built and
then removed.

Expose CPU-blocking intrinsic-like APIs:

```text
dcache_clean_all()      -> maintenance status
dcache_invalidate_all() -> maintenance status
```

Each operation sends one command. Command acceptance asserts `CACHE_MAINTENANCE_HOLD` before any later
instruction can retire. The CPU preserves its architectural state while held; I-cache, D-cache,
arbiter, and DRAM clocks continue. Hold is released only after the final required DRAM response, then
the intrinsic returns success or the recorded error from `CACHE_MAINTENANCE_STATUS`.

Do not physically gate the shared clock and do not implement a software polling loop. Give the CPU
core/fetch frontend an explicit synchronous hold input with priority over normal state transitions.
The core need not decode system-control device numbers: the system controller owns command decoding
and drives the generic hold. The D-cache also rejects CPU data requests while maintenance is active.

The intrinsic is a full compiler memory barrier: prior stores cannot move after the command and
subsequent cached accesses cannot move before hold is released. Do not expose a raw asynchronous start
API in the first revision. The write-back engine releases DRAM arbitration between bounded line bursts
so unrelated mandatory memory clients are not starved.

## Stage 9: integrate software ownership into GPU, DMA, and display APIs

The coherence model frozen in Stage 0 is deliberately non-coherent hardware plus explicit software
ownership. Stage 9 connects consumers to the Stage 8 global commands; it does not add or redesign
cache-core maintenance. Do not add range maintenance, per-line clean commands, automatic clean-after-
write, or GPU/DMA write snoops. I-cache invalidation remains single-cycle because I-cache lines are
never dirty.

The graphics, DMA, boot, and display helpers call the blocking intrinsic. A GPU command, DMA command,
segment handoff, or display swap cannot execute until required D-cache maintenance has released the
CPU. No caller implements its own polling loop.

### One-cycle delayed I-cache invalidation and the final jump

Preserve a registered one-cycle delay from the maintenance `DEVSEND` execute cycle to the actual
I-cache valid-bit clear and fetch-queue flush. The current system-control pulse already has this
registered shape; the fetch-pipeline revision must make it an explicit tested contract.

The boot handoff sequence is:

```text
D_INVALIDATE_ALL             dirty data is written back; wait until complete
ICACHE_INVALIDATE_ALL_DELAYED  arm the registered one-cycle-delayed invalidate pulse
final segment-switch jump    resolves while invalidate/queue flush takes effect
```

If a jump redirect and I-cache invalidation occur in the same cycle, the redirect target wins as the
new fetch PC, the fetch epoch advances, every old queued word is discarded, and any old outstanding
lookup or refill response is ignored. The compiler/backend must keep the invalidate and final jump
adjacent and must not schedule an ordinary instruction between them.

Stage0 performs this sequence before entering Stage1. Stage1 performs it before entering the
application. This is required once D-cache is write-back: raw D-cache invalidation at a boot boundary
would otherwise lose dirty handoff data.

### CPU, GPU, DMA, and display ownership contract

```text
CPU owns region
  -> CPU finishes writes
  -> D_CLEAN_ALL completes
  -> GPU/DMA/display owns region; CPU must not read or write it
  -> device commits all DRAM writes before reporting completion or accepting swap
  -> D_INVALIDATE_ALL completes before CPU reads a device-written region
  -> CPU owns region again
```

- GPU submission consumes or freezes the CPU-side buffer handle until the GPU completion is observed.
- A push-style graphics API should make CPU mutation unavailable after submission; a DMA copy into a
  GPU-exclusive region is an alternative when ownership cannot be transferred directly.
- GPU completion is visible only after its final DRAM write has completed, never merely after command
  execution has started.
- For CPU-rendered framebuffer tests, complete `D_CLEAN_ALL` before swap and do not modify that buffer
  until display ownership ends. The test-only API may enforce this manually.
- Display is a DRAM reader and does not snoop CPU cache writes. Double-buffer ownership and clean-before-
  swap provide its consistency.
- Software violations of ownership are programming errors; hardware is not required to repair them.

Acceptance:

- No cache maintenance ABI accepts an address or range.
- No external write causes automatic per-line cache invalidation.
- D-cache clean writes each dirty line exactly once and clears dirty only after DRAM completion.
- D-cache invalidate never loses dirty data and leaves every way invalid after completion.
- Stage0-to-Stage1 and Stage1-to-application handoffs preserve dirty data and fetch only new-epoch code.
- A GPU completion cannot precede the last accepted DRAM write response.
- CPU-to-GPU, GPU-to-CPU, and CPU-to-display ownership tests fail if maintenance or completion ordering is omitted.

## Stage 10: DRAM at twice CPU frequency

- Keep CPU/cache logic in the CPU clock domain and SDRAM command/data logic in the DRAM clock domain.
- Convert the synchronous refill buffer boundary into a proper asynchronous FIFO, mailbox, or equivalent proven CDC structure.
- Synchronize control tokens; never sample a shared 256-bit register array directly across domains.
- Carry line identity, beat order, `last`, and error state across the boundary.
- Analyze FIFO depth against refresh stalls, display/GPU service, and the 2:1 producer/consumer rate.
- First characterize CPU 50 MHz and DRAM 100 MHz before treating those clocks as accepted system targets.

## Stage 11 and 12: GPU integration and final closure

- Complete the final CPU, display, and GPU arbitration policy before frequency sign-off.
- Run full PnR after GPU integration; earlier 50/100 MHz results are characterization only.
- Final minimum target: CPU 50 MHz and DRAM 100 MHz.
- Stress concurrent I-cache refill, D-cache refill and write-back, GPU traffic, display deadlines, and SDRAM refresh.
- Capture durable board evidence in addition to simulation and PnR reports.

## Risk-scaled validation

Select the applicable checks for each stage and record both the checks run and any intentionally
skipped checks in the current implementation progress:

1. Update the Rust transaction model and bounded unit tests when architecture behavior changes.
2. Update RTL and bounded Verilog tests when hardware behavior changes.
3. Run the relevant crate tests for every code change.
4. Run affected external Icarus tests with explicit maximum cycle counts.
5. Run `scripts/validate-hardware.ps1 -Mode quick` for cross-layer or hardware integration changes.
6. Run audit/PnR when RAM geometry, clocking, resource use, or timing paths change.
7. Recheck BSRAM count and packing instead of weakening resource audits when memory changes.
8. Run board validation only when the stage requires physical evidence.
