# CPU V3 optimization history and roadmap

Status: living record of the current implementation and future optimization work
Repository: `../../../`
Updated: 2026-09-02

This roadmap lives inside the CPU V3 system crate so it stays with the system it describes; it is
maintained and committed together with each milestone. Benchmark workloads and runners live in the
system crate; generated per-stage CSV and presentation artifacts remain local unless explicitly
selected for the repository. Historical numbers below record the exact release-mode emulator runs
described by each Stage.

## Milestone workflow and document lifecycle

- Keep this document permanently as the optimization history and current-state handoff.
- Before and after each milestone, update the current implementation state here so another agent can
  resume from the documented repository state.
- Include the updated roadmap in the same commit that completes each milestone.
- Never leave the current architecture, benchmark, validation, PnR, timing, or resource summary stale
  after a CPU V3 optimization.
- Scale validation to the risk of the change. Correctness-focused changes with little timing or
  resource pressure may use a smaller targeted test set, provided the completed milestone records
  what ran and what was intentionally skipped. Changes that affect timing, resource use, memory
  geometry, clocks, or clock-domain crossings still require the corresponding hardware validation.

## Current implementation progress

| Stage | State | Result | PnR evidence at 54 MHz |
| --- | --- | --- | --- |
| 0 | Complete, 2026-08-29 | Removed per-line snoop/invalidate; froze global maintenance and boot-handoff semantics. | 4 BSRAM; 55.435 / 55.958 MHz |
| 1 | Complete, 2026-08-29 | Added private 256-bit refill buffers and complete-line commit. | 4 BSRAM; 57.127 / 62.440 MHz |
| 2 | Complete, 2026-08-29 | Replaced serialized reads with one real `8 x 32-bit` SDRAM burst. | 4 BSRAM; 55.327 / 56.090 MHz |
| 3 | Complete, 2026-08-29 | Split each cache into even/odd BSRAM banks; initialization contents are split by word parity and refill drain is eight cycles. | 6 BSRAM; 54.492 / 54.261 MHz |
| 4 | Complete, 2026-08-29 | Converted both caches to two ways with invalid-way-first deterministic victim replacement. The tag comparison now precedes the data-bank read, so a hit costs one more registered cycle until Stage 5 pipelines it. | 6 BSRAM; 61.425 / 56.530 MHz |
| System consolidation | Complete, 2026-08-30 | Folded the separate CPU V3 boot, SDRAM, and display systems into one fitted `cpu_v3_system`. This changed the full-system baseline to 7 BSRAM before the Stage 5 fetch pipeline work. | Full system: 9,740 Logic; 7 BSRAM; 57.345 MHz |
| 5 | Complete, 2026-08-30 | Pipelined resident cache reads for one accepted lookup per cycle and added a four-entry, epoch-tagged instruction fetch queue. Sequential ALU throughput now approaches two cycles per instruction. | Full system: 9,974 Logic; 7 BSRAM; 61.842 MHz |
| 6 | Complete, 2026-08-30 | Added demand-progress-triggered, low-priority next-line I-cache prefetch with redirect cancellation, discardable in-flight refills, simulation counters, and demand-safe cancellation races. | Full system: 10,129 Logic; 7 BSRAM; 54.538 MHz |
| 7 | Complete, 2026-08-30 | Added cycle-profiled emulator benchmarks, a redirect fast path, and an explicit dual-read RAM16 scalar register file. Hot control-transfer fetch waits fell from four cycles to two; the RAM16 register file cut 2,178 LUTs. | Full system: 8,046 Logic; 7 BSRAM; 57.549 MHz |
| 8 | Complete, 2026-08-30 | Split the production I/D caches, added write-allocate D-cache stores, dirty eviction, eight-beat SDRAM line writes, and blocking full-cache clean/invalidate with CPU hold and final status. | Full system: 9,548 Logic; 7 BSRAM; 54.918 MHz |
| 9 | Complete, 2026-08-31 | Added an exact related-clock 54/108 MHz gearbox. Cache/arbiter line traffic is 4 x 64-bit at 54 MHz; the Controller HS and SDRAM side remains 8 x 32-bit at 108 MHz. | Full system: 10,345 Logic; 7 BSRAM; CPU 54.965 MHz |
| 10 | Complete, 2026-08-31 | Replaced XOR/way-interleaved cache storage with two parity-split DPBs per cache. Refill and D-cache write-back now transfer directly as 4 x 64-bit beats without private 256-bit cache buffers. | Full system: 9,977 Logic; 4 DPB + 1 SDPB + 2 pROM; CPU 54.692 MHz |
| 11 | Complete, 2026-09-01 | Added a one-entry asynchronous store. A scalar store retires immediately and its data-port request/response runs in the background, overlapping ALU and other non-memory instructions; later memory operations wait on the single store buffer. | Full system: 10,025 Logic; 4 DPB + 1 SDPB + 2 pROM; CPU 56.51 MHz |
| 12 | Complete, 2026-09-01 | Added a conservative two-stage frontend: single-cycle integer ALU/immediate/control instructions overlap their fetch with the preceding execute, and a registered GPR forwarding path lets back-to-back dependent instructions observe the pending write. Loads, stores with a busy buffer, branches/jumps, devices, multiply, and FPU remain barriers. | Full system: 10,100 Logic; 4 DPB + 1 SDPB + 2 pROM; CPU 56.230 MHz |

Starting with System consolidation, PnR evidence is always taken from the complete `cpu_v3_system`
containing the CPU, boot path, SDRAM controller, and display path. Every subsequent completed stage
must record the Gowin PnR report's total `Logic` count alongside BSRAM use and Fmax. `Logic` is the
vendor report's aggregate logic-unit metric, not merely its LUT subtotal. The consolidation and Stage
5 counts above were reproduced from full-system builds at commits `3f62078` and `df4774a`; Stage 6
and Stage 7 use their corresponding full-system milestone builds. Stage 11 changed only the CPU store
path, and its full-system build closes the 54 MHz CPU clock at 56.51 MHz. Stage 12's full-system build
(commit `dabcb10`) closes the 54 MHz CPU clock at 56.230 MHz; the forward-compare/operand-mux/ALU/
registered-writeback bypass and the Execute-cycle request/response mux cost a little timing over
Stage 11, but the build still reports zero setup and hold TNS on every clock.

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
9. Run DRAM at exactly twice the CPU frequency through a related-clock 2:1 gearbox while keeping the rest of the system at 54 MHz.
10. Convert each cache's two data BSRAMs to true-dual-port parity banks and transfer cache lines directly as four 64-bit beats.
11. Let one scalar store complete asynchronously in the background while non-memory instructions continue to execute.
12. Overlap single-cycle integer execute with the next fetch through a conservative two-stage frontend and a single registered GPR writeback forwarding path.

Stages 0 through 12 are complete. Preserve the historical ordering above when interpreting old
measurements, and add future optimizations as separate numbered milestones rather than rewriting the
meaning of a completed Stage.

## Architectural decisions

- A cache line remains public and fixed at 16 physical 16-bit words, or 32 bytes.
- The cache/arbiter view of one line is four ordered 64-bit beats at 54 MHz; the physical controller view is eight ordered 32-bit beats at 108 MHz.
- I-cache and D-cache contain no private complete-line refill or write-back buffer.
- The current caches are two-way and use two true-dual-port parity BSRAMs per cache; Stage 10 removed
  the temporary complete-line refill and write-back buffers.
- CPU and controller clocks are exact related PLL outputs. The fixed 2:1 board gearbox is the width and clock boundary; it is not an arbitrary-ratio asynchronous FIFO.
- Two-way associativity, two-BSRAM banking, fetch pipelining, next-line prefetch, and write-back are separate milestones.
- The sixteen scalar registers are an explicit `CpuV3GprRam` module: synchronous-write,
  dual-asynchronous-read distributed RAM. Its synchronous write port commits one cycle after the
  retire; the Rust emulator models that delay exactly, and the Stage 12 forwarding mux exposes a
  matching pending write to a back-to-back dependent instruction.

## Historical stage records

Each Stage section below is a historical record: it describes the design, the measurements, and the
repository state at the time that Stage was implemented, not the current architecture. Where a Stage
section disagrees with the current-state sections above, the current-state sections take precedence.

## Stage 0: remove dead coherence machinery and freeze the final contract

Do this while D-cache is still write-through. It is a small deletion and contract change, not an
early implementation of write-back maintenance.

Audited state at the time, at branch `code-v0.3-dev`, commit `5775bf3`:

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
used 10,129 Logic units: 7 BSRAM, 80 RAM16 leaves, 8,936 LUTs, 713 ALUs, and 3,656 logic flip-flops.
The 54 MHz SDRAM/CPU clock closed at 54.538 MHz with zero setup and hold violations. On the
checksum-protected 2,048-word
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
- Promote the sixteen scalar registers from inferred cells to an explicit dual-asynchronous-read
  `CpuV3GprRam` distributed-RAM module, and model its one-cycle synchronous write commit in the Rust
  emulator.

Stage 7 validation used emulator/RTL cycle-by-cycle co-simulation, bounded Verilog regressions for
redirect issue, fall-through, backpressure, and stale epochs, the complete Icarus hardware suite, the
two-stage boot testbench, and a full Gowin rebuild plus current-artifact audit. The fitted system used
8,046 Logic units: 7 BSRAM, 88 RAM16 leaves, 6,749 LUTs, 769 ALUs, and 3,422 logic flip-flops. The
54 MHz SDRAM/CPU clock closed at 57.549 MHz with zero setup and hold violations.

The checksum-protected quicksort retired 708,531 words in 2,467,577 cycles (3.483 cycles per retired
word), saving 184,500 cycles, or 6.96%, from the Stage 6 baseline. Of 92,249 redirects, 92,244 hot
redirects waited exactly two cycles instead of four; the remaining five included cold I-cache misses.
The trace attributed 35.09% of all cycles to the data request/response path and 7.48% to fetch waits.
Its D-cache observed 152,482 loads, 44,457 write-through stores, and only 315 line refills, making
write-through store latency the next dominant optimization target. The full quicksort test runs in
release mode and is explicitly ignored by ordinary debug test runs.

The scalar register file was promoted from inferred cells to an explicit `CpuV3GprRam` module
(synchronous-write, dual-asynchronous-read distributed RAM), cutting 2,178 LUTs and 236 logic
flip-flops while adding eight RAM16 leaves and raising Fmax from 57.293 to 57.549 MHz. The FPU
register-file SSRAM claim dropped its `+8` inferred-cell fudge in favor of a precise split. The GPR
RAM's synchronous write port commits one cycle after the retire; the Rust emulator stages a
`gpr_write_enable`/`gpr_write_address`/`gpr_write_data` request and applies it at the start of the
next clock, matching the RTL exactly. The `sequential_alu_stream_reaches_two_cycle_throughput`
regression confirms the two-cycle-per-instruction target is unchanged, and a bounded
`verify_gpr_ram_with_iverilog` test covers the register file directly.

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

### Implementation decisions (2026-08-30)

- Split the I-cache and D-cache into separate modules. The I-cache becomes read-only: it drops the
  CPU-side write port, the write-through store path, and the `cpu_write`/`cpu_write_data` inputs. The
  D-cache gains write-back, dirty eviction, and the maintenance engine.
- Dirty state lives in a separate 128-bit SSRAM (two 64-bit words, one bit per way per set), not in the
  tag RAM. The maintenance scan uses a find-first-set / priority encoder over the dirty word, so the
  next dirty line is located in one cycle instead of scanning one set per cycle.
- Write-back and maintenance reuse the private 256-bit line buffer: the dirty/victim line is read from
  the data BSRAM into the buffer, streamed to DRAM as an eight-beat write burst, then the buffer is
  reused to receive the incoming line.
- Generalize the arbiter-to-SDRAM `memory_read_line` signal into `memory_line` (a line-transaction
  flag): `memory_write=0 && memory_line=1` is a line read, `memory_write=1 && memory_line=1` is a line
  write, and `memory_line=0` is a word transaction. The SDRAM adapter burst length becomes
  `(display || pending_line) ? 7 : 0`, and its write-data path streams eight 32-bit beats for a line
  write.
- The core stays a blocking execute machine: a write-back store hit completes in about two cycles
  (accept plus lookup), while a store miss read-allocates the full line first; `cpu_request_ready`
  drops exactly while the D-cache is busy (refill, eviction write-back, or maintenance). A posted-store
  write buffer is a separate follow-up, not part of this stage.
- CPU-issued `dcache_invalidate_all()` / `dcache_clean_all()` drive the same maintenance engine: the
  system-control device registers the command, starts the engine, asserts `CACHE_MAINTENANCE_HOLD`, and
  releases it with `CACHE_MAINTENANCE_STATUS` on completion. The I-cache invalidate stays single-cycle.

### Implementation result (2026-08-30)

Stage 8 is complete in the fitted `cpu_v3_system`. The production instruction-cache boundary is
read-only and ties off the proven internal cache engine's unreachable store inputs. The independent
D-cache implements write-allocate, dirty-victim write-back, and find-first-dirty global maintenance.
The arbiter and SDRAM adapter carry 32-bit line-write beats; DMA word writes remain in the low half.
The adapter captures all eight cache beats before issuing the SDRAM command, presents beat zero when
the command is acknowledged, then advances beats one through seven on consecutive controller cycles.

Focused RTL tests cover the read-only I-cache boundary, D-cache allocation/store/dirty eviction,
clean/invalidate semantics, line arbitration, and SDRAM word/read/write transactions. The full-system
RTL regression boots Stage0, Stage1, and both applications. Gowin PnR for the complete system reports
9,548 Logic, seven BSRAM blocks, and 54.918 MHz at the 54 MHz constraint. This passes with only
0.918 MHz margin and is narrow timing closure, not a robust frequency margin. Offline artifact audit
passes; physical board programming and observation of the controller's burst-write sampling remain
separate hardware validation.

## Stage 9: DRAM at twice CPU frequency

Implementation decision (2026-08-31): use exact related clocks, not an arbitrary-ratio asynchronous
FIFO. One PLL produces a 108 MHz Controller HS clock and its exact divide-by-two 54 MHz CPU/cache
clock. These clocks remain a timed related group; only the independent HDMI pixel domain is grouped
as asynchronous.

- CPU cache and memory-arbiter line ports are 64-bit at 54 MHz. One 256-bit line is four ordered
  beats; the existing eight-entry 32-bit cache buffers and parity-split BSRAM organization do not
  change.
- Controller HS and physical SDRAM remain 32-bit at 108 MHz. The board boundary pairs two read beats
  into one 64-bit CPU beat and splits one staged 64-bit write beat into two physical beats.
- A line write is staged completely in the 54 MHz domain before its SDRAM command is issued. The
  stable four-entry buffer is then consumed by the 108 MHz side; no changing 256-bit array is sampled
  across the boundary.
- Read pairs and command acknowledgements cross on tokens published at the controller-clock falling
  edge, leaving half a 108 MHz cycle before a related CPU-clock sampling edge. This is a fixed 2:1
  gearbox, not a general-purpose CDC FIFO.
- CPU/DMA word transactions continue to use the low 16 bits. Display remains an eight-beat 32-bit
  consumer; the shared adapter drains a completed four-entry 64-bit display buffer as eight words.

Acceptance additionally requires PnR timing at CPU 54 MHz / controller 108 MHz and physical board
validation of Controller HS read-valid phase and burst-write sampling. Simulation and PnR alone do
not prove those two vendor-controller timing details.

### Implementation result (2026-08-31)

The fitted system now derives related 108 MHz Controller HS and 54 MHz CPU/cache clocks from one
PLL. A board-level 2:1 gearbox holds commands, pairs two 32-bit controller reads into each 64-bit
CPU beat, and splits staged 64-bit line writes into two controller beats. CPU, caches, arbiter,
display scheduler, boot DMA, and system control remain in the 54 MHz domain. The HDMI pixel clock
remains independent and is the only domain grouped asynchronous to the CPU/controller clock group.

The display scheduler captures four 64-bit controller beats, then releases SDRAM ownership after
the normal recovery interval while the local buffer independently emits eight 32-bit display words.
An RTL test verifies that a CPU transaction is accepted before that display drain completes.

Stage 9's independent full-system PnR closes the 54 MHz CPU clock at 54.965 MHz. It uses 10,345 Logic
and seven BSRAM blocks; every reported setup/hold TNS is zero. CPU closure remains narrow. Physical
board confirmation of the vendor controller's read-valid phase and burst-write sampling is still
outstanding.

## Stage 10: dual-port cache BSRAMs and direct 64-bit line transfer

Replace each cache's two 1R1W parity/XOR-organized BSRAMs with two true-dual-port BSRAMs. Both ports
of a block always use the same mode in a cycle: either both read or both write. The cache never asks a
block to read and write in the same cycle.

- Remove the `bank = way XOR word_parity` mapping. Use `bank = word_index[0]`: bank 0 holds the eight
  even words of every line and bank 1 holds the eight odd words. Way is an ordinary address bit.
- Lookup mode uses the two ports of the selected parity bank to read the same word from way 0 and way
  1 concurrently. Tag comparison selects the returned way; no XOR reconstruction remains.
- Refill mode writes one 64-bit memory beat directly through all four 16-bit ports: bank 0 receives
  words 0/2 and bank 1 receives words 1/3 of that beat. Four cycles install the 256-bit line.
- D-cache write-back mode reads four words of the selected victim per cycle through all four ports and
  presents the resulting 64-bit beat directly to the memory interface. A synchronous-read prime phase
  is allowed before the request; thereafter four ordered beats stream without a private line buffer.
- Remove the I-cache refill buffer and D-cache refill/write-back buffer. Partial refills remain hidden
  by clearing/reserving the victim valid bit before the first direct write and committing tag/valid
  only after the fourth error-free beat. If 54 MHz timing requires it, add one explicit register at a
  named BSRAM-output or 64-bit memory-interface boundary rather than restoring a 256-bit line buffer.
- Store hits switch the selected BSRAM to write mode for that cycle. The cache pipeline must ensure no
  lookup read is required in the same cycle.

Acceptance:

- I-cache and D-cache still use exactly two 18-Kbit BSRAM blocks each.
- Every line is split evenly across the two blocks, with no way-dependent XOR mapping.
- Refill and write-back sustain one 64-bit beat per CPU cycle after any documented prime stage.
- No cache contains a private 256-bit refill or write-back register array.
- Replacement, dirty eviction, clean/invalidate, refill-error, and same-set way-selection tests pass.
- Full-system PnR still closes CPU 54 MHz and Controller HS 108 MHz; BSRAM packing and mode reports
  confirm the intended true-dual-port configuration.

### Implementation result (2026-08-31)

Each cache now owns one target leaf containing two inferred 1024 x 16 true-dual-port memories.
Bank 0 stores even words and bank 1 stores odd words; way is a normal address bit. Lookup reads both
ways through the two ports of the selected parity bank. Refill writes all four 16-bit ports directly
from each 64-bit beat. D-cache write-back uses one synchronous prime/capture boundary for beat zero,
then streams all four 64-bit beats without assembling a complete line in registers.

The I-cache emulator was updated to the same direct four-beat timing and passes cycle-for-cycle
Icarus comparison. Focused I-cache, D-cache, replacement, refill-error, dirty eviction,
clean/invalidate, and display/SDRAM simulations pass with bounded testbenches. Workspace quick
validation, strict Clippy, layering, source hygiene, boot regeneration, and byte-for-byte boot
repacking all pass.

The combined Stage 9/10 PnR reports 9,977 Logic (8,766 LUTs, 683 ALUs, 88 SSRAM cells), 4 DPB,
1 SDPB, 2 pROM, and 2 MULT18X18. The two I-cache DPBs and two D-cache DPBs are each attributed as
one two-block cache data leaf, so synthesis may legally pack/merge bank hierarchy without defeating
the exact two-BSRAM-per-cache resource audit.

## Stage 11: one-entry asynchronous store

Write-back stores and direct 64-bit line transfer still forced every scalar store to wait for its
data-port handshake before the core could fetch again. A store sat on the critical path, stalling the
whole blocking execute machine even though nothing yet read the stored value. Stage 11 lets one scalar
store finish in the background.

Design:

- Add a single-entry store buffer: `async_store_valid`, `async_store_issued`, `async_store_address`,
  `async_store_data`, and `async_store_fault_pc`, plus a new `ST_ASYNC_STORE_WAIT` core-FSM state.
  The emulator mirrors it as `AsyncStore` and the `Phase::AsyncStoreWait` enum variant.
- A store instruction (`opcode 0x9`) that finds the buffer empty retires immediately and returns to
  fetch. The buffered store owns the data port: `data_request_valid`, `data_write`, `data_address`,
  `data_write_data`, and `data_response_ready` all follow the outstanding store while it is in flight,
  in place of the normal pending-data path.
- Non-memory instructions (ALU, FPU, branches) continue to execute and retire while the store request
  is outstanding, hiding store memory latency behind useful work. The store no longer blocks its own
  instruction.
- Any later memory operation blocks: a second store, a load, or an FPU memory access waits in
  `ST_ASYNC_STORE_WAIT` until the single buffer drains, then it either enqueues a new store or advances
  to a normal data request. Memory ordering stays strict: at most one store is outstanding.
- `halted` is not asserted until the last buffered store is globally observed (`state == ST_HALTED &&
  !async_store_valid`), so a program that stores a result cannot expose its halt before the store
  commits.
- A store that gets an error response records `FAULT_DATA_MEMORY` with the buffered fault PC and the
  core transitions to fault only after the store response is accepted.

### Implementation decision (2026-09-01)

The core adds the buffer only when a store is retired and reuses the existing pending-data path to
hold a blocked store so it can become the next async store once the current one drains. The Verilog and
the Rust emulator share the same nonblocking timing: a freshly enqueued store cannot issue on the same
edge that created it, and a waiter observes completion one cycle after the response handshake. The
benchmark profiler now counts the fetch and data interfaces independently; the previous else-if chain
silently dropped every overlapped store.

### Validation (2026-09-01)

- Scenario 37 in `cpu_v3_core_tb.v` delays the data response, overlaps an `ADD` with a `STORE`, and
  checks that the ALU ran while the store was outstanding, that exactly two data requests occur, and
  that the final memory word holds the stored value.
- The bounded emulator test
  `emulator_async_store_overlaps_alu_and_blocks_next_memory_operation` asserts the store retires to
  fetch immediately, the overlapped ALU retires, and the following load parks in `AsyncStoreWait`.
- `data_probe_counts_overlapped_scalar_requests_and_latency` asserts that data requests equal the
  retired load-plus-store count rather than the earlier word-transaction proxy.

### Stage 11 results (commit `6789380`) against Stage 10 (`76d5bef`)

The 13-program suite is unchanged. Workloads that issue no store are bit-identical. Memory-heavy
workloads improve:

| Workload | Stage 10 cycles | Stage 11 cycles | Change |
| --- | ---: | ---: | ---: |
| int-short-memory | 3,415 | 3,214 | -5.9% |
| int-medium-memory | 75,038 | 70,750 | -5.7% |
| streaming-mix | 894,623 | 842,341 | -5.8% |
| quicksort-4096 | 5,217,959 | 4,989,362 | -4.4% |

The data-path cycles still dominate the memory-heavy cases: streaming-mix attributes 24.5% of its
cycles to the data request/response path and quicksort-4096 32.0%. A single store buffer is a partial
fix: it hides store latency only until a later memory operation needs the data port, so a deeper
store pipeline or a small write-combining buffer is the natural follow-up.

The Stage 11 full-system PnR reports 10,025 Logic, 4 DPB + 1 SDPB + 2 pROM, and closes the 54 MHz CPU
clock at 56.51 MHz.

## Stage 12: conservative two-stage fetch/execute overlap

Stage 5 pipelined the I-cache lookup and added a four-entry fetch queue, but the core was still a
blocking machine: it waited for the next word before retiring the current instruction. The Stage 11
commit history shows a roughly two-cycle-per-instruction floor for pure integer code (Stage 11
`int-medium-alu` was 2.50 cycles per retired word). Stage 12 overlaps ordinary single-cycle integer
execution with the next fetch so the core can accept a fresh instruction in the same cycle it retires
one, approaching one cycle per instruction without building a full execute pipeline.

Design:

- Keep the existing staged backend for every instruction that does not retire cleanly in one Execute
  cycle. Only a narrow class is promoted to the overlap path, so the change is a "limited two-stage
  frontend", not a pipelined CPU.
- Promote single-cycle, sequential-control-flow instructions to the overlap path:
  `opcode` `0`/`1`/`3..7` (ALU), `0xa` with `field_d != 8` (immediate; `field_d == 8` is the multiply
  barrier), the `0xe` control subset that is not itself a control transfer (`field_d <= 3`, `6`, `7`,
  `9..c`, `d` with `field_b <= 1`, `e` with `field_a == 1`), the `0xf` SETP prefix, and `0x9` stores
  only while the single async store buffer is empty.
- Keep as barriers: loads (`0x8`), stores with a busy buffer, multiply (`0x2`, `0xa/8`), FPU (`0xd`),
  branches/jumps (`0xb`, `0xe` fields `4`/`5`/`15`), devices (`0xc`), and the invalid instruction
  cases. These continue to use the exact pre-existing FSM transition.
- When a pipelineable instruction is in `ST_EXECUTE` and the queue has a response, the core drives
  `instruction_request_valid`/`instruction_response_ready` from Execute, loads the popped word into
  `instruction`/`instruction_pc`, advances `pc_register`, and stays in `ST_EXECUTE` instead of
  returning to `ST_FETCH_REQUEST`. If the queue is momentarily empty it falls to `ST_FETCH_RESPONSE`
  and resumes the original blocking path.
- The scalar register file keeps its registered synchronous write port. The read level is now a mux:
  `gpr_read_a_data`/`gpr_read_b_data` come from the pending `gpr_write_data` when
  `gpr_write_enable && gpr_write_address == <read>` and from the RAM async reads otherwise. This single
  bypass removes the earlier ordering requirement that back-to-back Execute cycles be two apart and is
  what makes a dependent `ADDI r0,r0+1` chain run at one cycle per instruction. The ALU result is still
  captured into the registered writeback; it is never driven combinationally into the GPR RAM write
  port, keeping the forward-compare/operand-mux/ALU/registered-writeback path off the timing-critical
  wrap.

### Implementation decision (2026-09-01)

The Verilog and the Rust emulator evolved together and share the same nonblocking timing. The
fetch-pipeline probe's sequential-ALU cycle target tightened from `<= 17` to `<= 10` cycles after the
first retire (the test now expects one instruction per cycle for a register-dependent chain, which
also exercises the forwarding bypass directly). `cpu_v3_core_tb.v` scenario 35/36 expected cycles were
reduced from 80/48 to 76/46 because the frontend no longer inserts a fetch bubble before the FPU
operations; these are timing updates, not numerical changes.

The cycle-profiled benchmark profiler changed its retirement attribution. The old model assumed the
instruction fetched before the next was also the one that just retired, which under the overlap path
counted every successor tag; that would mis-attribute, for example, 391 data requests against 256 real
loads. The profiler now keeps a small FIFO of frontend-accepted words and attributes a retire to the
oldest accepted word (popping per retired word, so a prefixed consumer retires two), and only flags a
redirect for genuinely control-transfer instructions whose resolved target differs from the fall-through
word.

### Stage 12 results (against Stage 11)

The same 13-program suite. Integer workloads gain the most; FPU-composed programs are nearly unchanged
because their steady state is dominated by the FPU pipeline barriers, not the fetch frontend.

| Workload | Stage 11 cycles | Stage 12 cycles | Change |
| --- | ---: | ---: | ---: |
| int-short-alu | 590 | 404 | -31.5% |
| int-short-branch | 2,244 | 1,508 | -32.8% |
| int-short-memory | 3,214 | 2,147 | -33.2% |
| int-short-mixed | 1,355 | 878 | -35.2% |
| int-medium-alu | 30,768 | 21,547 | -30.0% |
| int-medium-memory | 70,750 | 46,164 | -34.8% |
| streaming-mix | 842,341 | 594,894 | -29.4% |
| quicksort-4096 | 4,989,362 | 4,106,038 | -17.7% |
| int-icache-jump | 10,990 | 8,393 | -23.6% |
| fpu-short-add | 62 | 59 | -4.8% |
| fpu-short-mul | 63 | 60 | -4.8% |
| fpu-short-unary | 54 | 53 | -1.9% |
| fpu-long-mixed | 24,630 | 24,627 | -0.0% |

The geometric mean of the per-program cycle ratio is 0.774, a 22.6% cycle reduction across the suite.
`int-medium-alu` falls to 1.75 cycles per retired word (from 2.50) and `int-medium-memory` to 1.55
(from 2.38). The remaining head in integer programs is the data request/response path (quicksort-4096
still attributes 38.9% of its cycles to the memory path) and the branch redirect wait; those are the
natural next targets.

### Validation (2026-09-01)

- Release-mode emulator results above (not committed).
- `cargo test --workspace`: 410 passed; CPU V3 78 passed / 10 ignored.
- Verilog/RTL Icarus for the CPU V3 core, GPR RAM, and the fetch-pipeline probe passed with the new
  cycle contract.
- A cycle-accurate emulator-vs-Icarus co-simulation of the core now drives the same program through
  the Rust emulator and the RTL and compares a curated set of deterministic outputs every cycle
  (program counter, segments, retired words, halt/fault, and the instruction/data handshake), covering
  the Stage 12 overlap (a dependent `ADDI` chain at one instruction per cycle), the wide SETP-retire,
  a taken-branch redirect, the async store whose data value equals the forwarded `r0`, an FPU barrier,
  and the halt value. It runs as `core_emu_matches_rtl_pipeline_overlap` under `--ignored`.
- `halt_signal` was tightened from a live async GPR tap to a value latched at the HALT retire edge
  (mirrored in the emulator), so it is a stable architectural property that the co-simulation compares
  directly each cycle rather than only at the halt cycle.
- `scripts/validate-hardware.ps1 -Mode quick` and `-Mode iverilog` pass, including the two-stage flash
  boot signature testbench. This surfaced and fixed a pre-existing (Stage 9) mismatch: the
  `CpuV3System` top module exposes a 64-bit `sdram_write_data`/`sdram_read_data` gearbox interface
  while the signature testbench still modeled the 32-bit controller path, so its `.*` instantiation
  could not elaborate. The model now captures the four 64-bit line-write beats and returns line reads
  as four 64-bit beats, matching `DisplaySdramPort`.
- Full-system Gowin PnR (`cpu_v3_system`, commit `dabcb10`): 10,100 Logic (8,822 LUT, 750 ALU,
  88 SSRAM); 4,324 registers; 4 DPB + 1 SDPB + 2 pROM; 2 MULT18X18. The CPU clock closes at
  56.230 MHz against the 54.000 MHz constraint (2.23 MHz margin), and every reported setup and hold
  TNS is zero. `clk` and the display controller clocks close as expected. The tightest CPU path is the
  fetch-queue to I-cache way-valid route (0.734 ns slack), a Stage 5+/fetch-frontend path rather than
  the new forwarding/overlap logic. Board-level DRAM timing is unchanged from the Stage 9/10 gearbox
  and remains subject to the normal physical-boardside validation.

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
