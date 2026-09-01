# CPU V3 cache, fetch, and DRAM roadmap

Status: active handoff plan for future conversations
Repository: `D:\github\DigitalDesign-code`
Updated: 2026-08-29

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

- Stage 0: complete (2026-08-29). Removed the cache snoop ports, tied-zero wiring, architecture
  `invalidate_line` API, and per-line tests. Frozen the semantic global-maintenance ABI and DRAM-
  visibility ownership terminology; `D_CLEAN_ALL` and final status remain reserved and unimplemented
  until dirty state exists. Added semantic RCC cache APIs, represented the delayed I-cache invalidate
  plus JSEG as one terminal compiler IR operation, and updated both boot handoffs to perform
  `D_INVALIDATE_ALL`, prepare DSEG/registers, then emit adjacent
  `ICACHE_INVALIDATE_ALL_DELAYED; JSEG` words. Updated the generated Stage0/Flash baselines.
  Acceptance passed: forbidden-interface repository search was empty; workspace tests and strict
  workspace Clippy passed; layering, source-hygiene, and diff checks passed; cache and system-control
  Icarus tests passed; Stage0/Stage1 compiled-image adjacency and end-to-end boot tests passed; full
  Gowin PnR plus artifact audit passed. CPU V3 SDRAM and boot builds each still use four BSRAM blocks
  (2 SDPB + 2 pROM); at the 54 MHz constraint their reported Fmax is 55.958 MHz and 55.435 MHz with
  zero setup/hold TNS. No board programming was performed.
- Stage 1: complete (2026-08-29). Each cache now issues one aligned line request per read miss;
  the arbiter owns the SDRAM word port for the whole line, pairs sixteen word reads into eight
  ordered 32-bit beats (the low half of beat n is word 2*n) delivered into the cache's private
  flip-flop 256-bit refill buffer, and releases the port once the final beat is accepted; the
  cache then drains sixteen words into its data BSRAM privately and commits tag/valid only after
  a complete error-free line, so an error or invalidate can never expose a partially installed
  line. An error beat terminates the line response early and the arbiter recovers for the next
  request. Writes and the boot DMA stay single-word transactions; the SDRAM adapter still issues
  sixteen word reads per line (the real burst is Stage 2). The refill buffer carries an explicit
  `syn_ramstyle = "registers"` attribute after Gowin first inferred it as SSRAM. Fixed the stale
  stage1 manifest memory size (1346 -> 1332 bytes) left over from Stage 0 and the outdated
  "invalidate / snoop" labels in the structure diagram. Acceptance passed: cache and arbiter
  emu/NAND tests cover line streaming, beat pairing, backpressure, error termination, priority,
  and reset; the cache Icarus testbench verifies eight-beat refill, hits, write-through,
  invalidate, and error recovery; the full two-stage flash boot, display, and SDRAM system
  Icarus tests passed; workspace tests, strict workspace Clippy, layering, source hygiene, and
  the boot package byte-for-byte check passed; full Gowin PnR plus artifact audit passed. BSRAM
  use is unchanged at four blocks per system (2 SDPB + 2 pROM) with 56 RAM16 cells; at the 54 MHz
  constraint the boot and SDRAM builds report Fmax 57.127 MHz and 62.440 MHz. Board validation
  was intentionally skipped: no physical evidence is required for this stage.
- Stage 2: complete (2026-08-29). One aligned cache-line miss now produces exactly one SDRAM
  command sequence: the arbiter forwards a single line request and the adapter issues one
  ACTIVE + READ with burst length 7, streaming eight ordered 32-bit beats (with `last` and
  `error`) straight through the arbiter into the cache's refill buffer. The arbiter reverted to
  combinational request forwarding with ownership held from accept until the accepted beat
  carrying `last` (or any error beat); the Stage 1 word-pairing sequencer is gone. The word port
  (`TangNano20KSdramWordPort`) gained a `read_line` request flag, a 32-bit read bus, and
  `response_last`; word writes and word reads keep their held response. The display SDRAM
  adapter's CPU port gained the same line-burst path. Burst beats cannot be backpressured by the
  fitted Controller HS, so line beats are documented and tested as un-stallable stream beats; a
  due refresh never interrupts a burst (it waits for the transaction boundary). The system
  testbench SDRAM models now serve `burst_length + 1` beats and fatally reject any word read
  reaching the adapter. Acceptance passed: cache, arbiter (emu/NAND), word-port, and
  display-port tests cover the burst shape, beat ordering, `last`, write completion, priority,
  backpressure, and error release; the cache Icarus testbench still proves no partial line is
  installed on error; the two-stage flash boot, SDRAM, and display system Icarus tests passed
  (the boot test also asserts that no word read and at least one line burst reach the adapter);
  workspace tests, strict Clippy, layering, source hygiene, and the boot package byte-for-byte
  check passed; full Gowin PnR plus artifact audit passed. BSRAM use is unchanged at four blocks
  per system (2 SDPB + 2 pROM) with 56 RAM16 cells; at the 54 MHz constraint the boot and SDRAM
  builds report Fmax 55.327 MHz and 56.090 MHz. Board validation was intentionally skipped: no
  physical evidence is required for this stage.
- Stages 3-11: not started.

## Ordered major tasks

0. Remove the unused per-line snoop/invalidate direction now, freeze the global maintenance ABI and software ownership contract, and put I-cache invalidate immediately before the final boot jump.
1. Add one private 256-bit refill buffer between each cache and DRAM.
2. Change cache-line refill, the arbiter, and the SDRAM adapter to one real `8 x 32-bit` burst.
3. Change each I-cache and D-cache to two BSRAM blocks split by even and odd words, doubling internal bandwidth.
4. Change each I-cache and D-cache to two ways, including two tag lookups and a deterministic replacement policy.
5. Pipeline the I-cache hit path and add a small instruction fetch queue in front of the CPU.
6. Add low-priority next-line I-cache prefetch, reusing the 256-bit refill path.
7. Change D-cache to write-back and implement dirty eviction plus global clean/invalidate maintenance in the same change.
8. Integrate the already-frozen software ownership contract into DMA, and display APIs without changing the cache core again.
9. Run DRAM at twice the CPU frequency through an explicit asynchronous FIFO or equivalent CDC boundary.
10. (After GPU implementation) Complete GPU integration and final memory arbitration.
11. Close full-system timing at no less than CPU 50 MHz and DRAM 100 MHz, then run concurrent hardware stress tests.

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
- Define `D_INVALIDATE_ALL` as a cheap valid-bit clear while D-cache is write-through. When Stage 7
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

After Stage 7, the helper waits for `D_INVALIDATE_ALL` completion before preparing the final redirect;
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
- Prefer invalid ways before evicting a valid way.
- For D-cache write-back, dirty state is a later stage, not part of the first two-way conversion.
- Validate same-set alternating lines, invalid-way preference, replacement, invalidate, and refill failure.

## Stage 5: pipelined I-cache hit path and instruction fetch queue

This stage removes fixed hit latency. It is distinct from line prefetch.

Current hit timing for a simple ALU instruction is:

```text
cycle 1: FETCH_REQUEST
cycle 2: I-cache synchronous BSRAM lookup and tag check
cycle 3: registered cache response and instruction latch
cycle 4: EXECUTE and retire
```

Current simple-instruction throughput is therefore one instruction per four cycles when every access hits.

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

## Stage 7: D-cache write-back and global maintenance engine

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

## Stage 8: integrate software ownership into GPU, DMA, and display APIs

The coherence model frozen in Stage 0 is deliberately non-coherent hardware plus explicit software
ownership. Stage 8 connects consumers to the Stage 7 global commands; it does not add or redesign
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

## Stage 9: DRAM at twice CPU frequency

- Keep CPU/cache logic in the CPU clock domain and SDRAM command/data logic in the DRAM clock domain.
- Convert the synchronous refill buffer boundary into a proper asynchronous FIFO, mailbox, or equivalent proven CDC structure.
- Synchronize control tokens; never sample a shared 256-bit register array directly across domains.
- Carry line identity, beat order, `last`, and error state across the boundary.
- Analyze FIFO depth against refresh stalls, display/GPU service, and the 2:1 producer/consumer rate.
- First characterize CPU 50 MHz and DRAM 100 MHz before treating those clocks as accepted system targets.

## Stage 10 and 11: GPU integration and final closure

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
