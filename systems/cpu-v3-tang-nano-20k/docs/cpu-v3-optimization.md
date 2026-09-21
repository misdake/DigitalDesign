# CPU V3 optimization history and roadmap

Status: living record of the current implementation and future optimization work
Repository: `../../../`
Updated: 2026-09-20

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
| 6 | Complete, 2026-08-30; removed after measurement | Added demand-progress-triggered, low-priority next-line I-cache prefetch with redirect cancellation, discardable in-flight refills, simulation counters, and demand-safe cancellation races; an intermediate one-entry cancel-learning fix was folded into the removal. Later audit showed ~1.5% Logic / ~4.6% FF cost for +0.0003% frozen-suite benefit, so the whole mechanism was deleted again (see the Stage 6 section). | Full system: 10,129 Logic; 7 BSRAM; 54.538 MHz (at introduction) |
| 7 | Complete, 2026-08-30 | Added cycle-profiled emulator benchmarks, a redirect fast path, and an explicit dual-read RAM16 scalar register file. Hot control-transfer fetch waits fell from four cycles to two; the RAM16 register file cut 2,178 LUTs. | Full system: 8,046 Logic; 7 BSRAM; 57.549 MHz |
| 8 | Complete, 2026-08-30 | Split the production I/D caches, added write-allocate D-cache stores, dirty eviction, eight-beat SDRAM line writes, and blocking full-cache clean/invalidate with CPU hold and final status. | Full system: 9,548 Logic; 7 BSRAM; 54.918 MHz |
| 9 | Complete, 2026-08-31 | Added an exact related-clock 54/108 MHz gearbox. Cache/arbiter line traffic is 4 x 64-bit at 54 MHz; the Controller HS and SDRAM side remains 8 x 32-bit at 108 MHz. | Full system: 10,345 Logic; 7 BSRAM; CPU 54.965 MHz |
| 10 | Complete, 2026-08-31 | Replaced XOR/way-interleaved cache storage with two parity-split DPBs per cache. Refill and D-cache write-back now transfer directly as 4 x 64-bit beats without private 256-bit cache buffers. | Full system: 9,977 Logic; 4 DPB + 1 SDPB + 2 pROM; CPU 54.692 MHz |
| 11 | Complete, 2026-09-01 | Added a one-entry asynchronous store. A scalar store retires immediately and its data-port request/response runs in the background, overlapping ALU and other non-memory instructions; later memory operations wait on the single store buffer. | Full system: 10,025 Logic; 4 DPB + 1 SDPB + 2 pROM; CPU 56.51 MHz |
| 12 | Complete, 2026-09-01 | Added a conservative two-stage frontend: single-cycle integer ALU/immediate/control instructions overlap their fetch with the preceding execute, and a registered GPR forwarding path lets back-to-back dependent instructions observe the pending write. Loads, stores with a busy buffer, branches/jumps, devices, multiply, and FPU remain barriers. | Full system: 10,100 Logic; 4 DPB + 1 SDPB + 2 pROM; CPU 56.230 MHz |
| ISA 0.8 migration | Complete, 2026-09-11 | Breaking integer ISA revision (no new numbered Stage): destructive shift/multiply family at major 2, extended/system family at major 6, device access at major 7, symmetric branch/conditional-move/jump family at major B, `IMMHI12` renamed to neutral `PFX12`, `HALT` replaced by `SIGNAL r0, 0`, majors C/E reserved. Encoding, simulators, handwritten RTL, RCC backend, debugger decoding, and boot assets switched at the same boundary. Stage0 is 461 words (fits the 1024-word boot window); integer side keeps one `MULT18X18`. | Full system: 10,436 Logic; 4 DPB + 1 SDPB + 2 pROM; 2 x MULT18X18; CPU 55.597 MHz, zero setup/hold TNS |
| Single-stage boot merge | Complete, 2026-09-12 | Folded the former Stage1 into the BSRAM first stage: one image validates the descriptor and manifest and loads the reset-selected application, so the Stage1 image, the descriptor mirroring, and the duplicate DMA/UART/handoff code disappear. Container format version 4 reserves the former Stage1 descriptor fields; the error ABI uses stage `1` throughout and the boot-progress phases collapse to BOOT/DMA/APPLICATION. A manifest section-count bound closes the 16-bit `count << 5` wrap in the size check. | Merged Stage0 673 words (fits the 1024-word BSRAM boot window). Full system: 10,269 Logic; 4 DPB + 1 SDPB + 2 pROM; 2 x MULT18X18; CPU 54.522 MHz, zero setup/hold TNS |
| `ASR`/`ASRI` signedness fix | Complete, 2026-09-12 | Found on hardware: the display demo's negative sine/cosine offsets landed at +255 instead of -1. The handwritten RTL put `>>>` inside a conditional whose other branches were unsigned, and Verilog makes a `?:` unsigned when any branch is unsigned, so `ASR`/`ASRI` (and therefore `fix16::to_int()`) shifted logically. The shifts now compute in a statement-based `case` and the FSM selects the result; `fix16_to_int_rcc` co-simulates the conversion. | Full system: 10,232 Logic; 4 DPB + 1 SDPB + 2 pROM; 2 x MULT18X18; CPU 54.747 MHz, zero setup/hold TNS |
| Stage 6 prefetch removal | Complete, 2026-09-12 | Deleted the next-line I-cache prefetch entirely (offset-10 fetch-queue trigger, request/arm/cancel and cancelled-line learning in the I-cache engine, simulation counters) after the cost/benefit audit in the Stage 6 section showed ~4.6% register and ~1.5% Logic cost for +0.0003% frozen-suite benefit. The I-cache module sheds 31 REG / 178 LUT at synthesis; system PnR shifts the rest (unrelated modules move within normal re-optimization noise). Frozen stage-12 suite: +20 cycles out of 6,136,236; prefetch metric columns remain in the CSV schema, pinned to zero. | Full system: 10,321 Logic (4,196 FF); 4 DPB + 1 SDPB + 2 pROM; 2 x MULT18X18; CPU 56.141 MHz, zero setup/hold TNS |
| Cache RAM16 valid/victim + dirty window scan | Complete, 2026-09-12 | Moved both caches' valid and victim bits from flip-flops into a `CpuV3CacheValidRam` RAM16 leaf (asynchronous read, synchronous single-way write), with a one-set-per-cycle clear for global invalidation, reset, and memory-error scrub. The D-cache exposes `valid_sweep` and the system holds the core for the reset/scrub sweep (`sysctl_cpu_hold || valid_sweep`) so the core never sees a not-ready D-cache on its first post-reset access. The two-way hit expression drops its own invalidating gate to restore the tight I-cache way-valid depth. Revision audit: only the victim bit actually inferred RAM16 here, because the valid-array write ports depended on the cache's own read data and on the request handshake; both valid ways stayed as 128 flip-flops plus read multiplexers per cache until the cache valid-array write-port fix recorded below. Replaced the D-cache 128-bit dirty priority encoder with a 16-entry window scan overlapped with the in-flight write-back; the architecture `DataCache` selects lines way-major so the RTL and Rust wrapper stay bit-exact. Frozen-suite cycles unchanged per program (post-halt flush within 3 cycles). | Full system: 9,640 Logic (8,294 LUT, 770 ALU, 96 RAM16); 4,099 FF; 4 DPB + 1 SDPB + 2 pROM; 2 x MULT18X18; CPU 54.222 MHz, zero setup/hold TNS |
| Framebuffer 400x240 | Complete, 2026-09-12 | Widened the CPU framebuffer from 320x240 to 400x240 RGB565, so the 2x-scaled 800x480 mode fills the active window with no side border and the 3x 1280x720 mode leaves 40-pixel borders. The scanout line buffer grows to three 200-word slots (two 18-Kbit BSRAMs) with 10-bit addresses; the slot-base select keeps the address math off a synthesized DSP multiplier. The display demo re-centers on x=200 and uses 8/256 radians per pixel. | Full system: 9,706 Logic (8,354 LUT, 776 ALU, 96 RAM16); 4,098 FF; 5 DPB + 2 SDPB + 1 pROM; 2 x MULT18X18; CPU 56.304 MHz, zero setup/hold TNS |
| Scanout double buffer | Complete, 2026-09-12 | Collapsed the scanout line buffer from three 200-word slots to two. At two slots the producer only requests the bus once the consumer has freed a slot, when exactly one line is buffered, so the display is arbiter-urgent (`ready_count <= 1`) for every request and never round-robins with the CPU; the single line of lead covers SDRAM refresh plus the one in-flight transaction. Two 400-pixel lines are 12800 bits and fit a single 18-Kbit BSRAM (the three-slot 19200 did not). The host model's 0-underflow blackout ceiling drops from ~9900 to ~6400 cycles, so the line-buffer test pins 6000. | Full system: 9,706 Logic (8,361 LUT, 769 ALU, 96 RAM16); 4,088 FF; 5 DPB + 1 SDPB + 1 pROM; 2 x MULT18X18; CPU 54.298 MHz, zero setup/hold TNS |
| Cache valid-array write port | Complete, 2026-09-12 | The valid/victim RAM16 leaf had only ever inferred RAM16 for the victim bit. The D-cache selected the victim write way from the array's own asynchronous read data in `ST_LOOKUP`, and the I-cache put the `memory_request_ready` handshake into the valid write enable; Gowin then mapped the leaf's two valid ways as 128 flip-flops plus read multiplexers in every instance, costing 256 FF and about 840 LUT across the two caches while the 96 system SSRAM cells stayed fully accounted for by tags, the victim bit, and the GPR/FPU files. Both caches now invalidate the victim from the registered `pending_way` as the line request starts, which still precedes the first refill data beat, so no array write depends on that array's read data; each leaf instance infers twelve RAM16 cells with no flip-flops. The `CpuV3CacheValidRam` SSRAM claim becomes twelve cells per instance and the flip-flop dirty bitmap stops claiming SSRAM. The change is RTL-mapping only: no emulator or architecture-model source changed and `system_cosim` re-verifies RTL/model equivalence cycle by cycle, so the frozen-suite cycle counts are unaffected. | Full system: 9,025 Logic (7,585 LUT, 768 ALU, 112 RAM16); 3,834 FF; 5 DPB + 1 SDPB + 1 pROM; 2 x MULT18X18; CPU 55.904 MHz, zero setup/hold TNS |
| Resolved-target BTC | Complete implementation and validation, 2026-09-13; uncommitted, no new Stage | Default four-entry, two-word fully associative BTC with exact LRU, consumption-only fill and same-cycle target replay; optional eight-entry comparison and disabled control. Per-slot current metadata replaces toggled epochs so repeated redirects cannot revive old responses. Same 22 frozen programs and metric schema. See the BTC results below. | Full system, 4 entries: 9,408 Logic (7,872 LUT, 864 ALU, 112 RAM16), 4,103 logic FF, 6,688 CLS, 54.192 MHz, 0.066 ns setup slack. 8 entries: 9,637 Logic (7,968 LUT, 997 ALU, 112 RAM16), 4,340 logic FF, 6,958 CLS, 54.226 MHz, 0.077 ns setup slack. Both: 5 DPB + 1 SDPB + 1 pROM, two MULT18X18, zero setup/hold TNS. |
| FPU v2 S7b packed special path | Complete implementation and validation, 2026-09-20; no new Stage | Recovered the 54 MHz timing failure introduced by RCP/RSQRT without changing fetch behavior. The blocking special path registers its RF operand before normalization and stores each hidden-LUT interval as unsigned 17-bit `current` plus signed 10-bit `delta`, so one synchronous read replaces the former current/next pair. Aligned table bases remove address adders; the interpolator narrows to 10x9 bits; RCP uses a 33-bit saturating scale path while RSQRT uses its bounded non-saturating shift; one context copy replaces per-stage control replication. RCP/RSQRT return to T0..T3 with bit-identical reference results. Special-path synthesis falls from 528 LUT / 64 ALU / 162 FF / 1 DSP to 290 LUT / 17 ALU / 76 FF / 1 DSP. A registered-fetch-cursor probe reached 58.239 MHz / +1.348 ns but delayed redirect continuation, so it was removed after an ablation proved the packed special path closes timing alone; the four-file overlay remains in local stash `codex/fetch-timing-overlay-58.239mhz` for S7c contingency. | Full system: 9,247 Logic (7,621 LUT, 1,146 ALU, 80 RAM16), 4,189 logic FF, 6,756 CLS; 3 SDPB + 4 DPB + 1 pROM; 2 MULT18X18 + 1 MULT36X36 + 1 MULTADDALU18X18; CPU 55.058 MHz, +0.356 ns setup slack, zero setup/hold TNS. |
| FPU v2 S7c modular SINCOS | Complete implementation and validation, 2026-09-20; no new Stage | Added blocking SINCOS without changing fetch. Range reduction now feeds signed Fa and the positive 36-bit Q0.32 constant `K=round((2/pi)*2^32)=0xA2F9836E` into the existing three-stage shared MULT36X36. Taking `(Fa*K)>>>32` is bit-exact to the former `C0/C1` two-term reducer because `K=C0*2^16+C1`; it deletes four partial products, four reused accumulator registers and the serial reducer adder chain without adding a DSP. The local MULT18X18 remains dedicated to packed-LUT interpolation, followed by the result register that cuts the BSRAM -> DSP -> add -> RF path. Mode 00 SINCOS completes in T0..T7; mode 01/10 SIN/COS suppresses the second lookup/write and completes at T6. Dense +/-2pi and full-i32 tests retain 3-LSB maximum error. Special-path synthesis is 375 LUT / 34 ALU / 125 FF / 1 DSP. | Full system: 9,430 Logic (7,786 LUT, 1,164 ALU, 80 RAM16), 4,241 logic FF, 6,847 CLS; 3 SDPB + 4 DPB + 1 pROM; 2 MULT18X18 + 1 MULT36X36 + 1 MULTADDALU18X18; CPU 54.972 MHz, +0.327 ns setup slack, zero setup/hold violations. The tightest path remains a pre-existing core GPR write-data path; no special-path data path appears in the top 25. |
| FPU v2 compiler C0 | Complete, 2026-09-21 | Replaced the architectural Q8.8 model with the two-word Q16.16 encoding/decoder and a 64-scalar-register CpuV3Sim. Builder, decoder, simulator, and RTL field extraction are locked to one encoding contract; the special-function LUT/reference moved to the architecture layer. RCC freezes the Q16.16 source/ABI surface but explicitly rejects FPU operations until C1-C3, and the retired Q8.8 IR/emission is gone. Prescale helpers expose their shift and scaled result rather than claiming exact reconstruction; ACC accumulation is documented as modulo 2^64. The parked FPU display demo remains source-only while S2 temporarily selects the existing non-FPU boot application. | No RTL or bitstream input changed, so PnR was intentionally skipped. Workspace tests 574/0, core co-sim 22/0, system co-sim 2/0; strict clippy/fmt/layer/hygiene/docs pass. |
| FPU v2 compiler C1 | Complete implementation and validation, 2026-09-21 | RCC now lowers scalar Q16.16 construction, raw-half bridges, arithmetic, unary rounding, comparisons, calls/returns, parallel moves, and F-register spills through FLD/FST while preserving the frozen F0..F63 ABI. The core now executes AUX kind-00 GPR/F-register bridges through the existing external F ports and publishes scalar CMP flags to the transient pending test at retirement. A compiled-program co-simulation covers calls, a live-across-call spill/reload, comparison/branch, conversion and raw halves; the cycle-model memory harness now returns real read data instead of zero. Vector ranges, DOT/VMULS, special functions and constant integration remain C2/C3. | Full system: 9,681 Logic (8,004 LUT, 1,197 ALU, 80 RAM16), 4,285 logic FF, 7,082 CLS; BSRAM/DSP counts unchanged; CPU 57.116 MHz, +1.010 ns setup slack, zero setup/hold violations. The tightest path remains the core state-to-GPR-write-data cone; FPU data paths are absent from the top 25. Workspace tests 579/0, core co-sim 23/0, system co-sim 2/0; strict clippy/fmt/layer/hygiene/docs pass. |
| FPU v2 compiler C2 | Complete implementation and validation, 2026-09-21 | RCC now assigns contiguous register ranges to `vec2`/`vec3`/`vec4`, including packed FPU ABI arguments/results, range spills, cycle-safe vector moves and hardware partial-overlap constraints. Constructors, lane access, component arithmetic/unary operations, VMULS broadcasts, `fdot`, and vec4 import/export lower to the two-word VECTOR/AUX ISA. A compiled vector program crosses function calls and exercises VADD, VMULS, DOTSTORE, lane extraction and FLDV/FSTV in both core and full-system co-simulation. That test exposed and fixed a shared-multiplier ownership bug: a global return-valid pulse could select the multiply operand mux while a dot product was still issuing; path-local outstanding counters now own the mux through drain. The full-system boot example was also updated to match the temporary C0-C2 S2 placeholder instead of waiting forever for the parked display demo's UART frame. Special functions remain for C3; the frozen prescale library family is revisited there. | Full system: 9,689 Logic (8,011 LUT, 1,198 ALU, 80 RAM16), 4,285 logic FF, 7,009 CLS; BSRAM/DSP counts unchanged. This placement reaches 50.407 MHz with -1.320 ns worst setup slack (2 setup paths), while hold is clean. The two failing paths are the existing core state-to-GPR-write-data cone; FPU leaf data paths are not critical (the first FPU control endpoints have +0.421/+0.449 ns). Workspace tests 590/0, core co-sim 24/0, system co-sim 2/0; strict clippy/fmt/layer/hygiene/docs pass, and the corrected full-system boot example passes. |
| FPU v2 compiler C3 special/display unit | Complete implementation and validation; prescale decision remains open, 2026-09-21 | RCC now lowers `frcp`, `frsqrt`, `fsin`, `fcos`, and dual-output `fsincos` to the existing two-word SCALAR special encodings and preserves the contiguous two-register SINCOS result. The S2 display demo is restored, migrated from signed raw Q8.8 angle words to Q16.16, and exercises compiled SINCOS, multiply, rounding, integer conversion, cached framebuffer stores, clean, and vblank publication. A 20-million-step host run rendered both waveforms, the two-colour circle, and its phase marker. No FPU constant-table opcode was added: the design table is still proposed rather than frozen, and the real demo needs no such instruction. The first three prescale helpers are expressible with existing operations, but the frozen exact `v3_distance2_gt` contract needs an un-narrowed wide-ACC comparison that the ISA does not expose; the family remains explicitly rejected pending a split/relax/ISA decision. | The generated hardware is unchanged from C2: 9,689 Logic (8,011 LUT, 1,198 ALU, 80 RAM16), 4,285 logic FF, 7,009 CLS; BSRAM/DSP counts unchanged. Placement again reaches 50.407 MHz with -1.320 ns worst setup slack (2 setup paths), hold clean. Both failing paths remain the existing core state-to-GPR-write-data cone; the first FPU control endpoints retain +0.421/+0.449 ns. Workspace tests 591/0, core co-sim 25/0, system co-sim 2/0, and the full-system boot example pass; strict clippy/fmt/layer/hygiene/docs and reproducible boot packaging pass. |
| FPU v2 post-C3 AUX address timing repair | Complete implementation and validation, 2026-09-21; no new Stage | Captures the two-word pair's AUX/FLD/FST `X` field and its GPR-read select before entering the FPU states. The GPR address now starts from local registers instead of a live `ST_FPU2_WORD1/AUX_READ/AUX_APPLY` decode, without adding a pipeline stage or changing instruction latency. This removes the 19-level state -> asynchronous GPR read -> writeback cone that failed after C2; the generated cycle model remains externally identical. | Two independent fits reproduce 9,591 Logic (7,914 LUT, 1,197 ALU, 80 RAM16), 4,286 logic FF, and 6,951 CLS: -98 Logic, +1 FF, and -58 CLS versus C3; BSRAM/DSP counts unchanged. CPU timing closes at 55.966 MHz with +0.650 ns worst setup slack and zero setup/hold violations. The old path is absent from the top 25; the limiter is fetch queue head -> core state, and the first GPR write endpoint has +1.100 ns. Workspace tests 591/0, core co-sim 25/0, system co-sim 2/0, full Icarus/system example, strict clippy/layer/hygiene/docs, reproducible boot packaging, and artifact audits pass. |
| FPU v2 compiler prescale helpers | Complete implementation and validation, 2026-09-21 | RCC now lowers `v3_length2_shift`, `v3_length2_scaled`, `v3_normalize_safe`, and the relaxed `v3_distance_gt` entirely onto existing FPU v2 instructions. The distance helper accepts an ordinary Q16.16 threshold, prescales both inputs with one extra subtraction-headroom bit, narrows the squared length once through `DOTSTORE`, approximates the scaled distance as `s * RSQRT(s)`, and compares it with `threshold >> k`. Shift, narrowing, and special-function error make the boundary intentionally approximate; this removes the proposed raw-ACC compare/read extension. | No RTL or boot-program input changed, so PnR and the frozen benchmark suite were intentionally skipped. Workspace tests 594/0, core co-sim 26/0, system co-sim 2/0; strict clippy/fmt/layer/hygiene/docs and reproducible boot packaging pass. |
| FPU v2 small-range geometry helpers | Complete implementation and validation, 2026-09-21; supersedes the prescale API above | Replaced the prescale/CLZ/high-low/dynamic-shift library with direct small-range helpers: `v3_length2` is one `DOTSTORE`, `v3_normalize` adds `RSQRT` + `VMULS`, and `v3_distance_gt` uses `VSUB`, `DOTSTORE`, `s * RSQRT(s)`, then `CMP`. Release helpers have no guards. Matching `_checked` debug helpers halt with distinct signals when length/normalize components exceed inclusive +/-104, distance inputs exceed -16384..+16383, or a distance component exceeds +/-104. No opcode or RTL was added. | No RTL or boot-program input changed, so PnR and the frozen benchmark suite were intentionally skipped. Workspace tests 598/0, core co-sim 26/0, system co-sim 2/0; strict clippy/fmt/layer/hygiene/docs and reproducible boot packaging pass. |
| FPU v2 compiler and benchmark closure | Complete, 2026-09-21; FPU v2 line closed | Migrated the ten remaining Q8.8 benchmark sources to the final Q16.16 raw-half API while preserving their real inputs, loop counts, tiers, and cycle bounds. Full-width checksums now fold both halves; transform and particle state retain all 32 bits. Raw conversion is emitted directly at each call site, avoiding helper call/return overhead inside measured loops. Exact halt values were re-frozen and all 22 workloads pass. This is an explicit suite-identity revision, so performance results before and after it are not compared. | No RTL, ISA, compiler, or boot input changed, so PnR was intentionally skipped. Final frozen-suite performance is recorded under the new fingerprint in the project ledger; workspace validation and both mandatory co-simulations pass. |

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

The ISA 0.8 migration is deliberately not a numbered Stage: it is an architectural revision, not a
microarchitecture optimization. Its recorded numbers come from the post-migration full-system Gowin
build (`validate-hardware.ps1 -Mode all`, worktree after commit `b43ce4e`): 10,436 Logic (51% of the
fitted device), unchanged memory geometry (4 DPB + 1 SDPB + 2 pROM = 7 BSRAM blocks), and 55.597 MHz
on the 54 MHz CPU clock with zero setup/hold TNS on every clock. Integer DSP use stays at one
`MULT18X18` (the FPU keeps its own); the destructive multiply windows select [15:0]/[23:8]/[31:16]
from the same single multiplier through a constant-indexed part-select. Boot assets regenerated from
the same RCC sources: Stage0 461 words, Stage1 555 words, S1 application 79 words, S2 application
736 words; the Stage0 FNV-1a baseline is re-pinned in `build.rs`. A fresh run of the frozen
benchmark suite under the new ISA is reported separately, not folded into the Stage-to-Stage table.
The later single-stage boot merge (see the table) supersedes that asset set: the board now carries
one 673-word first stage and no Stage1 binary, and the FNV-1a baseline is re-pinned again.

The cache valid/victim RAM16 change and the D-cache dirty window scan are likewise not numbered
Stages. Their recorded numbers come from the full-system Gowin build after commit `148e63a`: 9,640
Logic (8,294 LUT, 770 ALU, 96 RAM16), 4,099 registers, unchanged BSRAM geometry (4 DPB + 1 SDPB + 2
pROM) and two `MULT18X18`, and 54.222 MHz on the 54 MHz CPU clock with zero setup/hold TNS. This
closure is narrow and dominated by core placement: the tightest path is the core's registered GPR
write, and the RAM16 leaves and the maintenance scan are not on it. Against the immediately
preceding build (`a956ca5`, before the scan) the 16-entry window scan removes 282 LUT; against the
pre-RAM16 `08bcc79` build the whole change removes 681 Logic and 97 registers at the cost of eight
RAM16 cells. The frozen suite reports identical per-program cycles, with the post-halt flush varying by at
most three cycles.

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

Stale-measurement notice: the system co-simulation suite commit exposed and fixed a
D-cache emulator timing drift — the Rust model inserted an 8-cycle read drain and an 8-cycle
write-back capture that the RTL never had. Emulator-side measurements in the Stage 8 through
Stage 12 sections (store-miss refill latency, dirty eviction/write-back latency, post-halt flush
cycles, and total cycles of D-cache-miss-heavy workloads) were recorded with the old model and are
inflated. RTL and hardware behavior were always correct. Do not mix these pre-fix latency figures
with post-fix numbers in the same comparison; the affected Stages have been rerun
with the corrected model through `benchmarks/run-history.ps1` (rows marked `reconstructed`).

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
uses the pulse to discard queued and outstanding old-path words (now using per-slot current bits). This preserves
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

## Stage 6: low-priority next-line prefetch — REMOVED after measurement

Stage 6 added a demand-progress-triggered, low-priority next-line I-cache prefetch:

- Trigger a candidate next-line request from real CPU fetch progress near the end of a line.
- Never recursively trigger another prefetch merely because a prefetched line completed.
- Give demand I-cache misses, D-cache traffic, display deadlines, and GPU demand traffic priority over prefetch.
- Reuse the I-cache 256-bit refill path when it is idle.
- Cancel an unissued prefetch immediately; an unavoidable in-flight burst may complete, but its result must be discardable.
- Track `issued`, `useful`, `useless`, and `dropped` counters in simulation or debug builds.

A frozen-suite regression then exposed one violation of the strictly-low-priority rule: in small
fully-cached loops the offset-10 trigger fired just before the loop's backward branch, and once
the prefetch burst was committed it could not be aborted (the shared SDRAM port protocol has no
cancel), so the redirect's own demand fetch — even a cache hit — stalled about seven cycles behind
the draining dead burst, every iteration. The fix (later folded into the removal) was one-entry negative
learning in the two-way cache: a prefetch dropped by `prefetch_cancel` recorded its line address,
and a later request for the same line was dropped at the port without issuing. In the frozen
stage-12 suite this recovered 6.8% of cycles on fpu-short-sincos and 4.7% on fpu-short-splat with
zero cycle change in every other program.

**Removal.** A later cost/benefit audit of the fitted design counted the prefetch-only logic at
roughly 196 flip-flops (~4.6% of the system's 4,229 logic registers, dominated by the 128
`way_*_prefetched` bits) and ~120-180 LUTs (~1.5% of 10,232 Logic), while the frozen stage-12
suite showed the mechanism was nearly inert: 16 issued and 12 useful prefetches across all 22
programs, and disabling it at the emulator cost +20 cycles out of 6,136,236 (+0.0003%). The
entire mechanism — the offset-10 trigger in the fetch queue, the prefetch request/arm/cancel and
cancelled-line learning logic in the I-cache engine, and the simulation counters — was therefore
deleted from the RTL and every bit-exact model. The demand refill path, the fetch queue itself,
and the two-way replacement are unchanged; the suite's prefetch metric columns are retained in
the CSV schema, pinned to zero.

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
  as four 64-bit beats, matching `SharedSdramPort`.
- Full-system Gowin PnR (`cpu_v3_system`, commit `dabcb10`): 10,100 Logic (8,822 LUT, 750 ALU,
  88 SSRAM); 4,324 registers; 4 DPB + 1 SDPB + 2 pROM; 2 MULT18X18. The CPU clock closes at
  56.230 MHz against the 54.000 MHz constraint (2.23 MHz margin), and every reported setup and hold
  TNS is zero. `clk` and the display controller clocks close as expected. The tightest CPU path is the
  fetch-queue to I-cache way-valid route (0.734 ns slack), a Stage 5+/fetch-frontend path rather than
  the new forwarding/overlap logic. Board-level DRAM timing is unchanged from the Stage 9/10 gearbox
  and remains subject to the normal physical-boardside validation.

## Follow-up fix: word writes and Controller HS burst protocol (2026-09-04)

The shared SDRAM port only asserted `controller_write_data_valid` for line writes
(`state == ST_WRITE_STAGE`), and the 54/108 MHz gearbox only filled its `write_buffer` on that
signal. A CPU/DMA word write therefore issued CMD_WRITE with `burst_length == 0` but never presented
write data; the controller sampled a stale `I_sdrc_data = write_buffer[0][31:0]`. The boot DMA's
Stage0-to-Stage1 SDRAM loads are word writes and were the first victim on physical hardware. All
three simulators masked the fault: the signature testbench SDRAM model committed word writes straight
from `sdram_write_data`, and the Rust `SdramModel` / `system_cosim_tb.v` committed
`pending_write_data` directly, so none exercised the gearbox starvation.

Fix:

- `SharedSdramPort` (display_sdram.v) now routes a word write through the full four-beat
  `ST_WRITE_STAGE`, holding the half-word (already placed for the DQM lane) on
  `controller_write_data` while `controller_write_data_valid` pulses all four beats, before
  ACTIVE/WRITE. The gearbox `write_buffer` is therefore always filled from capture index zero: the
  write-data stream is four beats for every write, preserving the free-running `write_capture`
  alignment without any gearbox change (a burst-zero write only reads entry zero). Line writes are
  unchanged except that `ST_WRITE_STAGE` now only advances through the line buffer when
  `pending_line`. The lane-positioned word is written to the existing 64-bit output register only
  on request acceptance and then held; recomputing it in `ST_OP_REQ` created a wide next-state mux
  that added 289 LUTs and reduced a deterministic 54 MHz build to 46.991 MHz.
- The Rust `SdramModel` and the `system_cosim_tb.v` behavioral port gained the same four-beat
  word-write `ST_WRITE_STAGE` transition, keeping the cycle-accurate co-simulation aligned with the
  RTL.
- `display_sdram_tb.v` gained a dedicated from-idle word write that asserts the staged data and
  `controller_write_data_valid`, plus a robust write-completion driver (`finish_cpu_write`) that acks
  the ACTIVE/WRITE commands wherever the port is in its sequence.
- Standalone 54/54 and 108/54 diagnostics exposed two additional edge-alignment faults. The
  diagnostic producer itself had driven staged write data combinationally from an already-advanced
  beat counter, rotating the four 64-bit pairs as `pair1, pair2, pair3, pair0`; it now registers
  pair 0 with the first valid cycle and advances one pair per cycle. More importantly, the production
  108/54 wrapper had held Controller HS `I_sdrc_cmd_en` until `O_sdrc_cmd_ack` and started its physical
  write stream only at that ack. The reference contract and board behavior show that `cmd_en` is a
  one-controller-clock request while `cmd_ack` is transaction completion. The wrapper now pulses the
  command once, keeps a separate in-flight flag, presents M0 on the WRITE command interval, and then
  advances through M1..M7. The same board characterization placed 108 MHz read data on phases 4..11;
  phase 3 is stale bus data and is no longer published.

Validation: crate lib/tests, `shared_word_and_burst_port_runs_in_iverilog`, the two-stage boot
signature testbench (which exercises DMA word writes), the system co-sim, and clippy pass. The fixed
standalone 108/54 diagnostic also passes its Icarus memory model; that model was enlarged after this
work found that its former last address was one word below the test region and had allowed unknown
values to mask failures. On the Tang Nano 20K, an interactive LED viewer independently inspected all
eight words through a burst read and eight burst-zero probes. Both the direct 54/54 path and the
production 108/54 gearbox returned M0..M7 exactly. Before moving the 108 MHz read window, the viewer
had shown `M6,M0,M1,M2,M3,M4,M5,M6` and constant stale M6 probes, directly identifying the early
sample. This is physical confirmation despite the separate UART/USB capture failure (the board-health
control also receives zero FPGA-UART bytes).

The production `cpu_v3_system` passes full-system Gowin PnR with the corrected shared wrapper at
10,099 Logic (8,822 LUT, 749 ALU, 88 SSRAM), 4,293 registers, 7,053 CLS, 4 DPB + 1 SDPB + 2 pROM,
and 2 MULT18X18. CPU Fmax is 54.386 MHz against the 54 MHz constraint (0.131 ns worst setup slack);
the 108 MHz controller clock reports 182.685 MHz Fmax, and all reported setup/hold TNS is zero. CPU
timing remains valid but narrow rather than robust. Gowin's project-wide warning output remains
enabled. Intentional RTL truncations are documented with source-line `gowin-lint: allow CODE`
markers, while the opaque Controller HS netlist warnings are filtered only by their exact file,
code, and internal-clock identity; other warnings remain visible. Any future GPU word/partial-line
write support must go through the same staged path.

## Follow-up: packed 2048-phase SINCOS lookup (2026-09-06)

The Q8.8 `FSINCOS` range reduction formerly rounded each input onto 1024 phase
steps and used one 16-bit ROM word per quarter-wave sample. Exhaustive software
comparison against host `sin`/`cos` found a two-LSB worst-case error. Merely
retuning the `2/pi` constant or biasing those 256 samples cannot reduce the
bound because one phase bucket can cover ideal results spanning three raw
Q8.8 values.

The final implementation uses 2048 phase steps and 512 quarter-wave samples.
Two unsigned 8-bit samples share each existing 16-bit sine word; samples 492
through the endpoint have the saturated Q8.8 magnitude 256 and reconstruct it
from the reflected sample index instead of storing a ninth bit. The reciprocal
and reciprocal-square-root regions keep their original 16-bit layout. Range
reduction uses the signed `MULT18X18` operand 83443 and rounds its product at
bit 16, which more closely approximates `4/pi` while remaining within the
18-bit multiplier input. The table is now an explicitly instantiated,
initialized DPB; its two synchronous ports read sine and cosine in parallel,
reducing `FSINCOS` from 12 to 9 execution cycles and from 13 to 10
fetch-to-fetch phases. Merely inferring two read-only ports caused Gowin to
duplicate the pROM, so the explicit primitive is required to keep both reads
in one physical block.

The architecture-level ROM error test checks every one of the 65,536 signed
Q8.8 inputs directly through the bit-exact arithmetic, without running the
device or system simulator. Worst error against a rounded Q8.8 host reference
is one LSB, worst continuous-component error is 0.003457, and RMS component
error is 0.001279. The canonical ROM regeneration audit, CPU V3 unit tests,
all eleven ignored CPU RTL/emulator checks (including a nonzero packed-high-half
SINCOS test), system co-simulation, and workspace quick validation pass with
bounded runs.

Full-system Gowin PnR reports 10,472 Logic (9,197 LUT, 747 ALU, 88 SSRAM),
4,294 registers, 7,127 CLS, 5 DPB + 1 SDPB + 1 pROM, and 2 MULT18X18. The CPU
clock closes at 57.917 MHz against 54 MHz with 1.253 ns worst setup slack; the
108 MHz controller clock reaches 175.186 MHz, and setup/hold TNS are zero. The
total BSRAM and DSP counts therefore remain seven and two respectively; the FPU
table changes one pROM into one DPB to expose both physical ports. An explored
18-bit packed ROM also preserved the block count but changed the pROM packing
mode, increased logic/congestion, and failed timing; it was not retained.

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

## Resolved-target BTC: measured 4/8-entry comparison

Implemented on `701c7f3+dirty`, without a new Stage number. The architecture and
handshake contract live in the CPU IP
[hardware-architecture.md](../../../ip/cpu-v3/docs/hardware-architecture.md);
configuration and diagnostic semantics live in the
[benchmark contract](../benchmarks/README.md#btc-comparison-diagnostics).
This adds no branch prediction, ISA changes, compiler changes or frozen workload revision.

All three configurations ran the same 22 programs with the same compiler/options
and exact halt checks. Program word counts and per-program retired instruction,
retired word and redirect counts agree. Per the benchmark contract's "Comparing
runs" rule nothing is added across programs: every aggregate below is a geometric
mean over per-program values, equal weight (counters that may be zero use the
geomean of value + 1, marked `(+1)`). Geomean characteristics are 12,851.7 retired
instructions/words and 1,214.0 redirects per program. The disabled
configuration reproduces the earlier suite's per-program cycles exactly.

| Runtime metric | BTC disabled | 4 entries (default) | 8 entries (comparison) |
| --- | ---: | ---: | ---: |
| Cycles (geomean) | 34,197.9 | 31,118.3 | 30,683.0 |
| Cycle ratio versus disabled (geomean of per-program ratios) | 1.0000 | 0.9099 (−9.01%) | 0.8972 (−10.28%) |
| Redirect wait cycles (+1) | 2,644.6 | 139.2 | 51.8 |
| Complete BTC hits (+1) / eligible restarts (+1) | — | 878.5 / 1,294.5 | 1,149.9 / 1,294.5 |
| Incomplete fills cancelled (+1) | — | 2.4 | 2.4 |
| Post-replay continuation wait cycles (+1) | — | 1.124 | 1.124 |

Eligible restarts include each program's initial stream, hence 22 more than the
retired-redirect count. Admission requires two consumed words: the ideal target-only
cache simulation is not the actual hardware hit rate. Timing changes also affect
cache/arbiter/refresh overlap, so cycle savings are not exactly the difference
of redirect-wait counters. The near-1 continuation-wait geomean rules out simply
moving most target bubbles to the first downstream word.

| Program | Disabled cycles | 4-entry cycles | 8-entry cycles |
| --- | ---: | ---: | ---: |
| long-quicksort | 1,562,747 | 1,385,568 | 1,378,815 |
| medium-binary-search | 225,385 | 192,696 | 185,544 |
| medium-dijkstra | 221,322 | 206,346 | 182,592 |
| frame-particles | 249,410 | 248,397 | 233,013 |
| frame-sprite-batch | 938,144 | 764,250 | 764,206 |

Four entries remain the default. Eight save another 1.40% on the geomean cycle ratio
(0.9860 versus four entries); the exact fitted costs are recorded only in the
milestone row above and generated reports.
Both builds pass 54 MHz constraints, but timing margin is narrow: the four-entry
limiter is the core GPR write-data path; the eight-entry limiter reaches the BTC
LRU write enable from the fetch queue head. These are fitted results, not physical
board validation or a guarantee that future placement changes will preserve margin.

Validation: workspace tests (498 passed), strict workspace clippy, layering,
source hygiene, documentation checks, reproducible boot packaging, all Icarus
hardware groups including Flash boot, the required CPU emulator/RTL tests and
system-level co-simulation, plus bounded independent full-address/data scoreboards
and RTL traces for 0/4/8 entries. Directed cases cover simultaneous restart/hit
acceptance, PC wrap, segment/high-address distinction, cache-line crossing,
backpressure/full reservations, stalled continuation, incomplete/error fills,
exact LRU eviction, reset/flush races, instruction-data changes after invalidation,
and stale responses surviving repeated redirects with same-cycle FIFO slot reuse.
Both capacities passed full-system Gowin PnR and `--check-existing` artifact audits.
No physical-board programming was performed; no commit or post-commit ledger row
was made.

Generated evidence remains untracked under `target/btc-analysis/`: `btc0.csv`,
`btc4.csv`, `btc8.csv` retain the frozen schema with explicit capacity labels;
each capacity directory holds per-program `summary.txt`, `btc.txt`, traces and
`aggregate.json`; `4/fit/` and `8/fit/` hold fitted reports. Validation logs are in
`target/cargo-summaries/`, with the aggregate quick/Icarus evidence under
`target/hardware-validation/`. The normal Gowin output is restored to the default
four-entry build after the eight-entry comparison.
