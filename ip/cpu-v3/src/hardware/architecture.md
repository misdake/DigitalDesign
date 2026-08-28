# CPU V3 hardware structure and timing

This document describes the revision 0.7 implementation, not an intended future
pipeline. The maintainable PlantUML source is in
[`cpu_v3_structure.puml`](cpu_v3_structure.puml).

## Execution model

`CpuV3Core` is a blocking, in-order, single-instruction machine. It does not
fetch another instruction while an integer memory request, integer multiply, or
FPU instruction is active. A successful instruction updates architectural state
and retirement exactly once. A fault updates neither retirement nor a partially
computed FPU destination; the documented four-beat store exception still keeps
memory writes acknowledged before a later beat faults.

With a one-cycle ready/valid responder, every instruction first visits
`FetchRequest`, `FetchResponse`, and `Execute`. The tables below count core
execute cycles from the `Execute` cycle through the retirement cycle. Add two
cycles for an ideal non-cached instruction fetch. Add arbitrary ready/valid wait
cycles for instruction or data memory.

## Core storage and execution resources

| Block | Current implementation | Per-cycle capability |
|---|---|---|
| GPR file | 16 x 16-bit registers | Combinational source reads and one or more decoded register updates at the clock edge |
| Control state | 16-bit PC, CSEG, DSEG, one IMMHI12 prefix, one transient three-way comparison | One instruction decoded; no speculative state |
| Integer ALU | 16-bit add/sub/logic/shift/compare plus CLZ and popcount | One non-multiply integer result |
| Integer multiplier | One registered signed 18 x 18 `MULT18X18` lane | Accepts an input each cycle, but the blocking core uses one operation at a time |
| FPR file | 16 registers x 4 signed Q8.8 lanes; 64 x 16-bit, two registered-address asynchronous reads and one synchronous write | Consume two issued lane reads and schedule the next addresses; commit one lane write |
| FPU lane ALU | One 17-bit saturating add/sub or one simple unary/move lane | Captures lane zero at dispatch, then writes one registered result and captures the next lane every cycle |
| FPU multiplier | One registered signed 18 x 18 `MULT18X18` lane | Primitive initiation interval is one cycle and latency is two cycles; four lane tags stream through a two-entry valid pipeline |
| FPU ROM | One synchronous 1024 x 16-bit BSRAM: 256 sine, 256 reciprocal, 512 reciprocal-square-root words | One lookup address and one registered result per cycle |
| Unary front/back end | One priority encoder, one registered 17-bit normalized mantissa, and one shared rounded variable shifter | Domain/exponent, normalization, index adjustment, and result scaling use separate short phases |
| ACC | Signed saturating 40-bit accumulator | One in-order product accumulation per cycle while the DOT pipeline drains |
| Transfer buffer | Four 16-bit words plus two transpose swap registers | Makes imports and overlapping rearrangements snapshot-clean |

The optional fitted system places separate 2 KiB instruction and data caches
around the core. Each cache is direct mapped with 64 sets and 16 words per line.
Stores are write-through and do not allocate on a miss. The caches, DMA, and
display share a single-outstanding SDRAM path through a system-owned arbiter.

## Integer instruction latency

| Operation class | Execute-to-retire cycles | Active phases |
|---|---:|---|
| ALU, immediate, compare, branch/jump, device, special-register, prefix, HALT | 1 | `Execute` |
| Integer `MUL` / `MULI` | 3 | `Execute -> MultiplyWait -> MultiplyCommit` |
| Integer `LOAD` / `STORE`, minimum | 3 | `Execute -> DataRequest -> DataResponse` |

## FPU instruction latency

The `FPU phases` column begins after the generic `Execute` cycle dispatches
opcode D. `Execute-to-retire` includes that generic dispatch cycle. Memory
latencies assume every request and response phase advances immediately.

| Operations | FPU phases | Execute-to-retire cycles | Current work per phase |
|---|---:|---:|---|
| `FSTORE`, `FACCSTORE`, `FCMP` | 2 | 3 | Execute operation, then commit/retire; for `FACCSTORE`, the synchronous FPR write lands on the retirement edge |
| `FLOAD` | 6 | 7 | Dispatch, then four serial lane writes (`x`, zero `yzw`), then commit |
| `FMOV`, `FADD`, `FSUB` | 6 | 7 | Capture lane zero at dispatch; four overlapped result-write/next-lane-capture phases; commit |
| `FABS`, `FNEG`, `FFLOOR`, `FCEIL`, `FROUND`, `FSAT01`, `FSIGN`, `FZERO` | 6 | 7 | Same one-lane-per-cycle schedule as vector add |
| `FMUL`, `FMULS`, `FDOT4ACC` | 7 | 8 | Issue four consecutive tagged DSP inputs, drain the two-cycle DSP pipeline in order, then commit |
| `FPACK4` | 10 | 11 | Dispatch, four snapshot reads, four writes, commit |
| `FUNPACK4` | 22 | 23 | Dispatch, four snapshot reads, sixteen lane writes/clears, commit |
| `FTRANSPOSE4` | 20 | 21 | Dispatch, six swaps x three phases, commit |
| `FRCP`, `FRSQRT` | 9 | 10 | Dispatch, registered domain/exponent, registered normalization, address, ROM lookup/wait, registered scale, write, commit |
| `FSINCOS` | 15 | 16 | Dispatch, angle multiply triplet, two ROM lookup triplets, four result writes, commit |
| `FIMPORT4`, minimum | 14 | 15 | Dispatch, four request/response pairs, four atomic destination writes, commit |
| `FEXPORT4`, minimum | 9 | 10 | Dispatch and four request/response pairs; the final response retires directly |

## Pre-pipeline 54 MHz timing baseline

These place-and-route measurements precede the lane-pipeline implementation
above and provide the baseline for the required post-change timing audit:

| System | Constraint | Actual Fmax | Worst setup slack | Worst-path class |
|---|---:|---:|---:|---|
| `cpu_v3_boot` | 54 MHz | 54.815 MHz | +0.275 ns | FPU state/address/SSRAM-read/next-state |
| `cpu_v3_sdram` | 54 MHz | 56.166 MHz | +0.714 ns | FPU state/address/SSRAM-read/next-state |
| `cpu_v3_display` | 54 MHz | 55.248 MHz | +0.418 ns | FPU state/address/SSRAM-read/next-state |

The first critical path is not a DSP path. It runs from an FSM state bit through
prefix/state decode, the FPR read-address mux, a RAM16 asynchronous read, FPU
operand/domain logic, and back into the next-state register. It has 18 logic
levels and roughly equal cell and routing delay. This means the SSRAM write port
is no longer the limiting path, but combinational address selection plus an
asynchronous read is now exposed inside the controller's next-state cone.

The next group starts at `fpu_operand_a` and ends at either
`fpu_rf_write_data` or `fpu_rom_index`. It passes through the shared rounded
variable shifter used by ROM normalization/scaling. The worst member has
0.406 ns slack. The multiplier lanes and BSRAM ROM output are not the present
critical paths.

## Post-pipeline timing results

The final RTL was routed both with the normal 54 MHz system constraint and with
a 60 MHz logic-clock characterization constraint. The latter changes only the
timing constraint used to guide placement; it does not change the checked-in
54 MHz SDRAM PLL or claim that the fitted SDRAM controller has been retimed for
a different physical clock.

| System | Normal 54 MHz Fmax | 60 MHz constrained Fmax | 60 MHz setup violations |
|---|---:|---:|---:|
| `cpu_v3_boot` | 57.217 MHz | 62.878 MHz | 0 |
| `cpu_v3_sdram` | 57.303 MHz | 60.241 MHz | 0 |
| `cpu_v3_display` | 57.661 MHz | 61.228 MHz | 0 |

All three constrained builds retain two `MULT18X18` cells and the SSRAM FPR
implementation. The FPR source carries an explicit `distributed_ram` synthesis
attribute so registered issue addresses cannot silently remap it into two
additional BSRAMs. The boot, SDRAM, and display reports each contain 56 RAM16
cells for the composed system and pass the existing resource audit.

At 60 MHz the old state/address/SSRAM/domain path is absent. The remaining
worst paths are the registered unary normalization/scale path or ordinary
integer decode/writeback, depending on placement. No DSP path is critical.

A follow-up change registers the unary magnitude in the domain-check phase, so
the shared variable shifter only ever reads registered inputs (the magnitude,
or the ROM output register). With that boundary the display system meets a
64.75 MHz logic-clock constraint (slack +0.313 ns, reported Fmax 66.091 MHz);
65 MHz fails with a single endpoint (Fmax 64.940 MHz, reproducible). The
remaining critical cone is the ROM commit phase itself (input mux, barrel
shifter, saturation into the result register), with the integer
register-to-register writeback path close behind at roughly a 66-68 MHz
equivalent delay.

## Implemented lane pipeline

The add/move/simple-unary loop overlaps these independent operations:

1. Capture operands for lane `n` from the asynchronous FPR ports.
2. In the same cycle, compute and schedule the FPR write for lane `n - 1` from
   the registered operands.
3. Let the synchronous RAM commit the previously scheduled write at the edge.

Lane zero is captured in `FpuExecute`, then four compute/write phases overlap
the remaining captures. This reduces the complete FPU portion of a four-lane
add/move/simple unary from ten to six cycles without adding an ALU
combinational path. Destination/source aliasing is safe because adjacent lanes
have different RAM addresses.

`DspMulS18` has initiation interval one and two-cycle latency. A two-entry
valid/tag shift register accepts one lane every cycle and associates each
returned product with its destination lane. Vector multiply rounds and
schedules one result per cycle after the fill. DOT consumes the same ordered
product stream through the 40-bit saturating ACC feedback path. `FMULS`
snapshots its scalar before issuing lane zero, preserving `Fa == Fb` behavior.

| FPU operation | Before pipelining | Current FPU phases |
|---|---:|---:|
| add/sub/move/simple unary | 10 | 6 |
| multiply/multiply-scalar/dot | 14 | 7 |

These counts retain blocking instruction retirement. They improve
intra-instruction lane throughput; they do not allow another CPU or FPU
instruction to enter the pipeline.

### Continuation `k`

The architectural `continuation_mask()` calculation exists in the Rust numeric
model and is tested, but neither the RTL nor the Rust hardware FSM uses it to
bound a lane loop. Every vector operation currently executes four lanes.

The present two-read-port SSRAM cannot derive all four lanes' continuation bits
combinationally without extra reads. The appropriate implementation is a
non-architectural 4-bit nonzero mask per FPR. Update one bit whenever the single
FPR write port commits, derive `k` from that mask, and latch the source `k` value
when the instruction starts so overlapping writes cannot change its range.
This is cached derived data, not ABI or spill state, and reset must clear it in
lockstep with the FPR RAM.

The useful bounds are:

| Operation | Lane bound |
|---|---|
| add/sub | `max(k_a, k_b)` |
| multiply and dot | `min(k_a, k_b)` |
| multiply-scalar | `0` if scalar is zero, otherwise `k_a` |
| zero-preserving simple unary | `k_a` |
| move | at least `max(k_source, old_k_destination)` so old destination tails are cleared |
| scalar RCP/RSQRT/SINCOS/FCMP | fixed scalar semantics; do not use `k` to suppress execution |

Continuation-bound execution is deliberately deferred. The current work keeps
the four-lane schedule and adds no `k` metadata or architectural state.

### Timing implementation

1. FPR read addresses are now registered when generic `Execute` dispatches an
   FPU instruction. Gather, transpose, export, and lane pipelines schedule the
   following address while consuming the current asynchronous data, removing
   the state mux in front of RAM16 without adding latency.
2. Domain decisions now use a latched unary operand; that phase captures the
   exponent and the absolute magnitude, so the shared variable shifter only
   ever reads registered inputs. The following phase registers the normalized
   mantissa before endpoint/exponent adjustment. Together these boundaries add
   only one cycle to RCP/RSQRT while removing both observed long combinational
   cones.
3. The ROM scale result is now registered between the shared barrel shifter and
   FPR write-data. This adds the second RCP/RSQRT cycle and does not change
   numerical behavior.
4. No DSP stage was added because none of the post-change reports names a DSP
   input, product, or rounding endpoint as critical.

The lane pipeline is primarily a latency/throughput optimization. Registered
FPR issue addresses and the two unary boundaries are the changes that removed
the original Fmax bottlenecks.
