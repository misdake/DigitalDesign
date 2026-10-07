# CPU V3 optimization index

Status: concise milestone index
Repository: `../../../`
Updated: 2026-10-02

Add exactly one short sentence here for each completed optimization; append implementation detail,
measurements, rejected alternatives, and validation evidence to
[`cpu-v3-optimization-record.md`](cpu-v3-optimization-record.md). Current architecture and fitted
numbers belong in [`architecture.md`](architecture.md); exact resource and benchmark ledgers are
generated outside this document.

## Completed milestones

- 2026-08-29 — Stage 0 removed per-line coherence machinery and froze global cache-maintenance and boot-handoff semantics.
- 2026-08-29 — Stage 1 added private 256-bit cache refill buffers with complete-line commit.
- 2026-08-29 — Stage 2 replaced serialized SDRAM reads with one real eight-word burst.
- 2026-08-29 — Stage 3 split both caches into even/odd BSRAM banks.
- 2026-08-29 — Stage 4 converted both caches to deterministic two-way set associativity.
- 2026-08-30 — System consolidation combined CPU, boot, SDRAM, and display into one fitted system.
- 2026-08-30 — Stage 5 pipelined cache hits and added a four-entry epoch-tagged instruction fetch queue.
- 2026-08-30 — Stage 6 added next-line I-cache prefetch, then removed it after measuring negligible benefit for material cost.
- 2026-08-30 — Stage 7 added benchmark profiling, a redirect fast path, and a RAM16 scalar register file.
- 2026-08-30 — Stage 8 introduced split I/D caches, write-allocate D-cache stores, dirty eviction, and global maintenance.
- 2026-08-31 — Stage 9 introduced the exact-related 54/108-MHz SDRAM gearbox.
- 2026-08-31 — Stage 10 replaced cache line buffers with parity-split true-dual-port BSRAM banks and direct 64-bit transfer.
- 2026-09-01 — Stage 11 added a one-entry asynchronous scalar-store buffer.
- 2026-09-01 — Stage 12 overlapped single-cycle integer execution with fetch and added registered GPR forwarding.
- 2026-09-11 — ISA 0.8 migrated the integer encoding, toolchain, simulators, RTL, debugger, and boot assets together.
- 2026-09-12 — Single-stage boot folded the former second boot stage into the BSRAM-resident loader.
- 2026-09-12 — The ASR/ASRI repair removed conditional-expression signedness corruption found on hardware.
- 2026-09-12 — Cache valid/victim state and the D-cache dirty scan moved from wide FF logic toward RAM16 and windowed scanning.
- 2026-09-12 — The framebuffer widened from 320x240 to 400x240 RGB565.
- 2026-09-12 — Scanout collapsed from three line slots to a two-slot single-BSRAM buffer.
- 2026-09-12 — Cache valid-array write ports were restructured so both valid ways inferred RAM16 instead of FF read muxes.
- 2026-09-13 — The resolved-target BTC added a four-entry exact-LRU redirect cache with same-cycle replay.
- 2026-09-20 — FPU v2 S7b packed the RCP/RSQRT special path and restored 54-MHz timing.
- 2026-09-20 — FPU v2 S7c added modular SINCOS range reduction and lookup without another DSP.
- 2026-09-21 — FPU v2 compiler C0 froze the Q16.16 model, encoding, simulator, and source ABI.
- 2026-09-21 — FPU v2 compiler C1 added scalar lowering, calls, spills, comparisons, and GPR/F-register bridges.
- 2026-09-21 — FPU v2 compiler C2 added vector allocation, operations, calls, spills, DOT, and VMULS.
- 2026-09-21 — FPU v2 compiler C3 added special-function lowering and restored the compiled display demo.
- 2026-09-21 — The post-C3 AUX address repair removed the failing asynchronous GPR-read control cone.
- 2026-09-21 — FPU v2 prescale geometry helpers proved the API before being superseded by direct small-range helpers.
- 2026-09-21 — Small-range geometry helpers replaced the temporary prescale API without adding an opcode.
- 2026-09-21 — FPU v2 closure migrated and re-froze all 22 benchmark workloads under the final Q16.16 API.
- 2026-09-22 — RCC gained segregated heap free lists, checked vectors, and segmented application placement.
- 2026-09-22 — ISA 0.9 added paged DSEG mapping and asynchronous LCOPY/DCLEANL/DWAIT line operations.
- 2026-09-23 — GPU fixed-line bring-up connected submit, dummy rendering, tile-linear scanout, and the main SDRAM arbiter.
- 2026-09-23 — GPU variable line bursts extended its memory masters and SDRAM path to 32–128-byte transactions.
- 2026-09-23 — Generic device watch-change lets the CPU sleep until a selected device channel changes.
- 2026-09-23 — The blocking GPU framebuffer cache added eight tile entries, LOAD/CLEAR, masked writes, and 128-byte cleaning.
- 2026-09-23 — Partial GPU tile waves added tile-local gradients and three staggered left-to-right update fronts.
- 2026-09-23 — GPU memory-path repair restored D-cache RAM16 mapping and reduced the long-write gearbox to one 64-bit pair.
- 2026-09-26 — The viewport rasterizer joined the GPU tile cache, and a registered tile-corner result restored 54-MHz timing.
- 2026-09-26 — Four swizzled GPU cache DPBs removed pixel beat read-modify-write, added explicit retirement ACK and K=1 acquisition, and passed full-frame integration, 54-MHz timing, and triangle-only cold-boot validation.
- 2026-09-27 — CPU B-source predecode and two-stage pixel AABB improved fitted timing with unchanged ISA and pixel results; offline and loaded-UART validation passed, with cold boot pending.
- 2026-09-27 — Compact raster ownership, shared edge stepping, stable BSRAM inference and shared cache/control RAM16 ports reduced Logic; tile wins at both quad rates, and the two-pixel default passed complete Flash Verify and loaded UART, with cold boot pending.

- 2026-09-27 — Four-row display buffering in two BSRAMs removed response staging and added linear RGB565-to-sRGB conversion; the completed-pair RAM16 FIFO is retained after whole-system comparisons, with mutually exclusive 2x/3x build features.

- 2026-09-28 — Stream-owned fetch offsets, owner-qualified response broadcast and synchronous I/D-cache tag DPBs reduce whole-system Logic while preserving cache cycles and GPU behavior; matched RAM16, staging and arbitration alternatives remain in the local resource audit.

- 2026-09-28 — Unified FPU lanes, common integer operands/results, synchronous D-cache metadata, ordered fetch cursors and six-client round robin reduce Logic while retaining two-way caches, dense clean throughput and frozen-suite execution cycles; full offline validation passed.
- 2026-09-28 — The native SDRAM controller and two-pair 54/108-MHz bridge integrate bank-striped, naturally aligned line bursts with scalar masking; full-system simulation and routed timing pass.
- 2026-10-07 — The shared FPU multiplier uses two registered stages, shortening MUL/VMUL/VMULS and the dot family by one beat while retaining SINCOS timing with an 18-bit phase register; the [record](cpu-v3-optimization-record.md#2026-10-07--two-stage-fpu-multiplier) preserves the frozen-suite comparison and offline validation boundary.

## Next work

Before the GPU v2 reset, the framebuffer storage/quad attachment to the display work-1 baseline
completed full integration regression and routed resources recorded in
[`architecture.md`](architecture.md#current-fitted-result-and-validation-boundary).

The historical [eight-DPB framebuffer component](framebuffer-cache.md) reduces LUT use in a matched
standalone comparison and removes the numerical color queue. Its contract and tests are complete;
bank-order execution with one latched lane-control bit further reduces selection logic while
preserving word-level memory/render concurrency. Production storage/quad integration retains
blocking color traffic and initializes local Z to far depth. Depth/blend execution, depth
surface binding and concurrent sector scheduling remain pending; whole-system fit is recorded
in the architecture document.

Future CPU or system optimizations are added here only after completion; planned GPU work is tracked
in the GPU design documents and the project todo rather than expanded in this index.

The 2026-10-02 development baseline integrates independent GPU v2 Rust components,
their explicit research alternatives and the shared vendor SDRAM service while
retaining serial production memory and an inactive GPU shell; current offline
qualification and the physical boundary are described in [architecture](architecture.md).

2026-09-28: source-held D-cache writeback removes duplicate FF payload storage and shares arbiter
payload qualification; details are in the append-only record and current fit in
[`architecture.md`](architecture.md#current-fitted-result-and-validation-boundary).
