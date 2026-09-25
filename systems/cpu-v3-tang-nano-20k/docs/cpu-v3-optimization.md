# CPU V3 optimization index

Status: concise milestone index
Repository: `../../../`
Updated: 2026-09-26

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

## Next work

Future CPU or system optimizations are added here only after completion; planned GPU work is tracked
in the GPU design documents and the project todo rather than expanded in this index.
