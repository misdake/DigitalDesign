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
| FPR file | 16 registers x 4 signed Q8.8 lanes; 64 x 16-bit, two asynchronous reads and one synchronous write | Read two selected lanes; commit one lane write |
| FPU lane ALU | One 17-bit saturating add/sub or one simple unary/move lane | Current FSM alternates operand capture and result scheduling, so useful throughput is one lane per two cycles |
| FPU multiplier | One registered signed 18 x 18 `MULT18X18` lane | Primitive initiation interval is one cycle and latency is two cycles; current FSM waits and drains each lane before issuing the next |
| FPU ROM | One synchronous 1024 x 16-bit BSRAM: 256 sine, 256 reciprocal, 512 reciprocal-square-root words | One lookup address and one registered result per cycle |
| Unary front/back end | One priority encoder and one shared 17-bit rounded variable shifter | Normalization and result scaling use separate phases but the same shifter |
| ACC | Signed saturating 40-bit accumulator | One product accumulation on each current multiply-commit phase |
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
| `FMOV`, `FADD`, `FSUB` | 10 | 11 | Dispatch, four pairs of operand-capture/result-write phases, then commit |
| `FABS`, `FNEG`, `FFLOOR`, `FCEIL`, `FROUND`, `FSAT01`, `FSIGN`, `FZERO` | 10 | 11 | Same fixed four-lane capture/write schedule as vector add |
| `FMUL`, `FMULS`, `FDOT4ACC` | 14 | 15 | Dispatch, four `wait/settle/commit` multiplier triplets, then FPU commit |
| `FPACK4` | 10 | 11 | Dispatch, four snapshot reads, four writes, commit |
| `FUNPACK4` | 22 | 23 | Dispatch, four snapshot reads, sixteen lane writes/clears, commit |
| `FTRANSPOSE4` | 20 | 21 | Dispatch, six swaps x three phases, commit |
| `FRCP`, `FRSQRT` | 7 | 8 | Dispatch, normalize, address, ROM lookup, ROM wait, scale/write, commit |
| `FSINCOS` | 15 | 16 | Dispatch, angle multiply triplet, two ROM lookup triplets, four result writes, commit |
| `FIMPORT4`, minimum | 14 | 15 | Dispatch, four request/response pairs, four atomic destination writes, commit |
| `FEXPORT4`, minimum | 9 | 10 | Dispatch and four request/response pairs; the final response retires directly |

## Measured 54 MHz timing limits

The current high-frequency place-and-route reports have no setup or hold
violations:

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

## Optimization assessment

### Lane pipelining

Lane pipelining is not implemented. The add/move/simple-unary loop intentionally
alternates a read-capture cycle and a compute/write-schedule cycle. These two
operations use independent paths and can be overlapped safely:

1. Capture operands for lane `n` from the asynchronous FPR ports.
2. In the same cycle, compute and schedule the FPR write for lane `n - 1` from
   the registered operands.
3. Let the synchronous RAM commit the previously scheduled write at the edge.

Capture lane zero in `FpuExecute`, then overlap four compute/write phases with
the remaining captures. This reduces the complete FPU portion of a four-lane
add/move/simple unary from ten to six cycles without adding an ALU
combinational path. Destination/source aliasing is safe because adjacent lanes
have different RAM addresses. This is the best first latency optimization.

The multiplier is also pipeline-capable but currently underused. `DspMulS18`
has initiation interval one and two-cycle latency, while the FSM consumes three
states per lane before issuing the next. Separate issue and retirement lane
counters plus a two-bit valid/tag shift register can accept one lane every
cycle. Vector multiply can then round and schedule one result per cycle after
the fill. With lane zero issued in `FpuExecute`, the expected four-lane FPU
portion is seven cycles instead of fourteen. DOT can consume one product per cycle through the 40-bit saturating
ACC dependency, subject to a new PnR check of the accumulator feedback path.

| FPU operation | Current FPU phases | Proposed pipelined phases, four lanes | Proposed phases with `k` active lanes |
|---|---:|---:|---:|
| add/sub/move/zero-preserving unary | 10 | 6 | `2` when `k=0`, otherwise `k+2` |
| multiply/multiply-scalar/dot | 14 | 7 | `2` when `k=0`, otherwise approximately `k+3` |

These proposed counts retain blocking instruction retirement. They improve
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

Implement lane pipelining before `k`. Pipelining gives a deterministic benefit
for every vector and introduces the issue/retire counters that `k` can later use
as its terminal lane. Adding metadata first would complicate the current
two-phase loop and then be rewritten by the pipeline change.

### Timing improvements

No extra architectural pipeline stage is required at 54 MHz. If more margin or
a higher core clock is required, apply changes in this order and rerun the boot,
SDRAM, and display PnR profiles after each step:

1. Register or predecode FPR read addresses when the generic `Execute` state
   dispatches an FPU instruction, so the large state mux is not in front of
   RAM16. Keep gather, transpose, and export addresses in explicit issue
   registers. Each multi-cycle phase can schedule the next address while it
   consumes the current asynchronous data, so this need not add latency.
2. Ensure domain decisions use a latched unary operand rather than allowing an
   asynchronous FPR value to feed the global next-state cone. This may add one
   cycle only to RCP/RSQRT if address predecode alone is insufficient.
3. Add a `RomScale` result register between the shared barrel shifter and FPR
   write-data register if the 0.406 ns path becomes negative. This adds one
   cycle to RCP/RSQRT and does not change numerical behavior.
4. Do not add a DSP stage unless a new report names a DSP input, product, or
   rounding endpoint. The current report does not justify it.

The lane pipeline is primarily a latency/throughput optimization. Address
predecode and unary operand isolation are the changes that directly target the
current Fmax bottlenecks.
