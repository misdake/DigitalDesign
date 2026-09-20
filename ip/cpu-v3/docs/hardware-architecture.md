# CPU V3 hardware structure and timing

This document describes the revision 0.8 ISA running on the current Stage 12
microarchitecture, not an intended future pipeline. The maintainable PlantUML source is in
[`cpu_v3_structure.puml`](cpu_v3_structure.puml).

## Execution model

`CpuV3Core` is precise and in order, with a conservative two-stage frontend rather
than a general pipeline. A single-cycle instruction with sequential control flow
may accept the next queued word in its Execute cycle and remain in Execute, so an
eligible sequence can retire one instruction per cycle. Loads, integer multiply,
FPU instructions, control transfers, device operations, invalid cases, and stores
while the one-entry asynchronous store buffer is busy remain barriers on the
original staged path. At most one instruction retires per cycle. A successful
instruction updates architectural state and retirement exactly once. A fault
updates neither retirement nor a partially computed FPU destination; the
documented four-beat store exception still keeps memory writes acknowledged
before a later beat faults.

The fitted system places a four-entry instruction fetch queue in front of the
core. It reserves fetched and outstanding words and uses per-slot current bits:
a redirect or flush marks all older requests stale, and their ordered responses
are drained without delivery. New requests win when a slot is drained and reused
in the same cycle. This remains correct across arbitrarily many redirects before
an old response returns. Sequential word addresses wrap PC without carrying into
CSEG. A queued word or matching memory-response bypass can be accepted directly
in FetchRequest or in the Stage 12 pipelineable Execute subset; backpressured
responses are queued.

The queue also contains a fully associative resolved-target BTC, defaulting to
four entries of two 16-bit instruction words (not necessarily two instructions).
It compares the full physical target using a 22-bit tag plus a zero check on the
upper address bits. A restart hit supplies the first word combinationally and
resumes downstream requests at the target's second successor. A replay cursor
advances only when the core accepts a word, independently of downstream readiness.
The ordinary queue may hold later words during replay; BTC consumption never
pops that queue or frees its capacity. A hot two-cycle I-cache continuation can
therefore follow both BTC words without a bubble. Downstream refusal or a miss
can still stall the continuation.

A miss collects only the first two successful core acceptances of that stream;
installation occurs after both words arrive. Redirect, flush or an instruction
error cancels an incomplete fill without evicting a complete entry. Empty slots
are used first, followed by exact LRU replacement; accepting the first BTC word
marks its entry MRU. Reset, halt, fault and global I-cache invalidation clear the
BTC and replay/fill state. Ordinary redirects retain complete entries. The BTC
neither predicts a target nor adds memory traffic to complete a partial entry;
software-controlled instruction coherency applies exactly as to the I-cache.

`CPU_V3_BTC_ENTRIES=0|4|8` is a build-time environment setting, default 4.
The CPU crate build script generates one constant used by both the Rust model
and generated Verilog. Zero disables BTC storage/lookup for comparison while
retaining the per-slot stale-response fix. There are no new hardware ports or
ISA controls. The tables below count Execute through retirement; fetch waits
are additional when neither the queue, BTC nor bypass can supply the word.

## Core storage and execution resources

| Block | Current implementation | Per-cycle capability |
|---|---|---|
| GPR file | Explicit 16 x 16-bit distributed-RAM leaf with dual asynchronous reads and one synchronous write | One registered write per cycle; a forwarding mux exposes the pending write to a matching next instruction |
| Control state | 16-bit PC, CSEG, DSEG, one PFX12 prefix, one transient three-way comparison | One instruction decoded; no speculative state |
| Integer ALU | 16-bit add/sub/logic/shift/compare plus CLZ and popcount | One non-multiply integer result |
| Integer multiplier | One registered signed 18 x 18 `MULT18X18` lane | Accepts an input each cycle, but the blocking core uses one operation at a time |
| FPU front-end | Two-word instruction latch: word0 exposes `Fa`/`Fb` (or AUX `X`/`Fa`) and drives the RF read addresses immediately; word1 carries `Fd`/subop/len/mode | One `instr_complete` pulse per pair; FPU instructions are fetch barriers and never consume `PFX12` |
| FPR file | Two mirrored 512 x 32 SDPB BSRAMs giving 2R1W; architectural `F0..F63` are addresses `0..63`, the hidden LUT region is `64..511` (mirror A: RCP and SINCOS; mirror B: RSQRT even/odd) | Two registered-address synchronous reads and one broadcast write per cycle; a same-cycle write/read returns the old word on both ports |
| FPU scalar/vector ALU | One combinational 32-bit Q16.16 ALU (add/sub/min/max/abs/neg/floor/ceil/round/trunc, all wrapping) shared lane-serially by the scalar and vector paths | One lane per cycle; the vector path sequences 2/3/4 consecutive lanes |
| FPU multiplier | One shared inferred 36 x 36 pipe (four 18 x 18 lanes), three stages, II = 1, with a 9-bit destination tag | The multiply, dot and SINCOS range-reduction owners time-share it; the core serializes FPU instructions |
| ACC | Signed 64-bit Q32.32 accumulator | One exact product enters per cycle; accumulation wraps modulo 2^64, and `DOTSTORE` narrows `ACC[47:16]` once |
| Special path | Blocking RCP/RSQRT/SINCOS controller over the hidden BSRAM tables, one local inferred 18 x 18 interpolation lane, and the shared pipe for SINCOS range reduction | RCP/RSQRT `T0..T3`; SINCOS `T0..T7` dual, `T0..T6` single; owns both RF read ports while active |

The integer `ASR`/`ASRI` instructions shift `signed(rd)` arithmetically. The RTL computes the
shift result in a statement-based `case` and the FSM selects it; it never places `>>>` inside a
conditional expression, because Verilog makes `?:` unsigned when any branch is unsigned and would
silently turn the arithmetic shift into a logical one. `FTOI16` and every other signed FPU result
depend on the same rule for any arithmetic shift in their datapath.

The AUX kind-00 integer bridges (`ILO2F`/`IHI2F`/`I16TOF`/`FLO2I`/`FHI2I`/`FTOI16`) are performed
by the core through the unit's external F read/write ports, since only the core can reach the GPR
file. A scalar `CMP` publishes its registered `flag_lt`/`flag_eq`/`flag_gt` into the core's
transient pending test when the pair retires, so a following conditional branch or conditional
move consumes it exactly like `CMPS`/`CMPU`.

The optional fitted system places separate 4 KiB instruction and data caches
around the core. Each cache is two-way set-associative with 64 sets and 16 words per line.
Two true-dual-port BSRAMs split every line strictly by word parity. During lookup,
the two ports of the selected parity bank read the same word from way 0 and way 1;
the parallel tag comparison selects the corresponding registered bank result.
While that resident read resolves, the next lookup may start, allowing one
ordered hit request and response per cycle when there is no miss, invalidate,
write, or response backpressure. The instruction cache exposes only reads. The
data cache is write-back: stores allocate on a miss and set a dirty bit in a
separate flip-flop bitmap, not in a RAM leaf, because the maintenance scan reads
a 16-entry window of both ways every cycle; replacing a dirty victim first
writes its complete line. Valid and victim bits live in a RAM16 leaf with
asynchronous reads: each cache keeps its two valid ways and the victim bit in
twelve 16-deep cells. That inference only holds while no array write selects its
way or enable from the same array's asynchronous read data, so both caches clear
the victim from the registered pending way when the line request starts. A
global invalidate, reset, or memory-error scrub clears one set of both ways per
cycle, and the system holds the CPU for that sweep.
A read or write-allocate miss issues one aligned line request; the system arbiter
streams four ordered 64-bit beats at 54 MHz. Each beat writes four words directly
through the four BSRAM ports, and tag/valid state commits only on the fourth
error-free beat. The victim is invalid throughout refill, so an error or invalidate
cannot expose a partial line. Dirty eviction primes the synchronous DPB outputs,
then streams four ordered 64-bit beats without a private line buffer. The board
gearbox pairs/splits those logical beats against the 32-bit SDRAM controller at
the related 108 MHz clock. The boot DMA keeps
single-word transactions. Full D-cache clean and clean-plus-invalidate scan the
128-bit dirty bitmap one 16-entry window per cycle, overlapped with the
in-flight write-back while the CPU is held; there is no per-line snoop
interface. The system-control I-cache invalidation pulse is
registered for one cycle so the compiler's adjacent invalidate-and-JSEG
handoff resolves deterministically.

## Integer instruction latency

| Operation class | Execute-to-retire cycles | Active phases |
|---|---:|---|
| Pipelineable ALU, immediate, compare, major-2 shifts, non-control major-6 operations, non-halting `SIGNAL`, prefix, and store with an empty async buffer | 1 | `Execute`, optionally accepting the next queued instruction in the same cycle |
| Branch/jump, device, `SIGNAL` type 0 (halt), and other single-cycle barriers | 1 | `Execute`, then restart through the fetch path |
| Integer `MUL0`/`MUL8`/`MUL16`/`MULI` | 3 | `Execute -> MultiplyWait -> MultiplyCommit` |
| Integer `LOAD`, minimum | 3 | `Execute -> DataRequest -> DataResponse` |
| Integer `STORE` with an empty async buffer | 1 to retire | The buffered data request/response continues in the background; a later memory operation waits for it |

## FPU v2 scheduling and latency

FPU v2 has no scoreboard, forwarding, or dependency comparison. Each operation
belongs to a fixed timing profile, and the core starts the next FPU instruction
only when the current instruction's `R_WAIT`, `W_WAIT`, and required `X_WAIT`
countdowns have all reached zero. The two-word front-end is a fetch barrier:
word0 is accepted in `Execute`, word1 through the normal instruction port, and
the pair retires as two words.

- Scalar ALU, vector ALU, `VMUL`/`VMULS` and `MOV` run one lane per cycle; the
  vector read window is `T0..T(last_lane)` and the writes trail by two beats.
- The shared 36 x 36 pipe has latency 3 and II = 1, so `VMUL`/`VMULS`/scalar
  `MUL` write back three beats after the lane's operands are captured.
- `DOT`/`DOTADD`/`DOTSTORE` accumulate the complete signed product of every
  lane into the 64-bit Q32.32 ACC with no per-lane narrowing; `DOTSTORE`
  narrows once and clears ACC.
- RCP/RSQRT are blocking `T0..T3`; SINCOS is `T0..T7` for the dual
  `sin`/`cos` output and `T0..T6` for a single output. The special path
  monopolizes both RF read ports while active and reuses the shared pipe for
  its one SINCOS range-reduction product.
- `FLD`/`FST` and their vector forms reuse the core data port with its existing
  variable-latency handshake; loads stay blocking until the destination
  registers are written, while stores drain through the core's early-release
  store buffer after the source values are captured.

The hidden BSRAM tables are generated from one Rust reference model in the
architecture crate (`fpu_lut`), which also backs the architectural emulator and
the host error test, so the emulator and the initialized RTL cannot disagree.
RCP/RSQRT are measured at 1.38/1.47 result-ulp and SINCOS at 3 LSB over the
whole `i32` input range, inside the frozen 2/2/4-ulp targets.

## Current fitted-system result

The complete `cpu_v3_system` is fitted and routed, including the CPU, boot path,
caches, 54/108-MHz SDRAM gearbox, and display path. The fitted numbers live in
[the system architecture document](../../../systems/cpu-v3-tang-nano-20k/docs/architecture.md),
which owns them and is where they are updated; they are deliberately not repeated
here.

What that fit says about the CPU itself: the tightest CPU-clock path is the
core's registered GPR write, not the hidden special-function lookup or the cache frontend,
and the D-cache dirty write enable is the second tightest class, which is why the
maintenance scan reads the whole-word bitmap instead of moving it into an
addressed RAM leaf.

The following timing sections are retained as implementation history for the FPU
lane pipeline. They are not the current full-system Stage 12 result.

## Historical pre-FPU-pipeline 54 MHz baseline

These place-and-route measurements precede the lane-pipeline implementation
above and provide the baseline for the required post-change timing audit:

| System | Constraint | Actual Fmax | Worst setup slack | Worst-path class |
|---|---:|---:|---:|---|
| `cpu_v3_system` (full system) | 54 MHz | 54.815 MHz | +0.275 ns | FPU state/address/SSRAM-read/next-state |

The former `cpu_v3_sdram` and `cpu_v3_display` harnesses were folded into
`cpu_v3_system` when the CPU V3 systems were consolidated; this row is the
surviving full-system measurement.

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

## Historical post-FPU-pipeline timing results

The final RTL was routed both with the normal 54 MHz system constraint and with
a 60 MHz logic-clock characterization constraint. The latter changes only the
timing constraint used to guide placement; it does not change the checked-in
54 MHz SDRAM PLL or claim that the fitted SDRAM controller has been retimed for
a different physical clock.

| System | Normal 54 MHz Fmax | 60 MHz constrained Fmax | 60 MHz setup violations |
|---|---:|---:|---:|
| `cpu_v3_system` (full system) | 57.217 MHz | 62.878 MHz | 0 |

The consolidated full-system build retains two `MULT18X18` cells and the SSRAM
FPR implementation. The FPR source carries an explicit `distributed_ram`
synthesis attribute so registered issue addresses cannot silently remap it into
two additional BSRAMs. The boot report contains 56 RAM16 cells for the composed
system and passes the existing resource audit.

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

## Revision 0.7 fix16 lane pipeline (historical)

> This section and the two below describe the **retired** blocking Q8.8 FPU
> pipeline (`FLOAD`/`FMOV`/`FPACK4`/`FUNARY`, 16-bit-vector FPR, 40-bit
> saturating ACC, continuation `k`). The current two-word Q16.16 FPU v2 is
> described in [FPU v2 scheduling and latency](#fpu-v2-scheduling-and-latency).
> The numbers are kept only as timing history.

The add/simple-unary loop overlaps these independent operations:

1. Capture operands for lane `n` from the wide asynchronous FPR reads (both
   read addresses stay parked on Fa/Fb for the whole instruction).
2. In the same cycle, compute and schedule the FPR write for lane `n - 1` from
   the registered operands.
3. Let the synchronous RAM commit the previously scheduled write at the edge.

Lane zero is captured in `FpuExecute`, then four compute/write phases overlap
the remaining captures. This reduces the complete FPU portion of a four-lane
add or simple unary from ten to six cycles without adding an ALU combinational
path. Destination/source aliasing is safe because per-lane write enables update
only the lane just computed while later lanes are still being read.

`DspMulS18` has initiation interval one and two-cycle latency. A two-entry
valid/tag shift register accepts one lane every cycle and associates each
returned product with its destination lane. Vector multiply rounds and
schedules one result per cycle after the fill. DOT consumes the same ordered
product stream through the 40-bit saturating ACC feedback path.

| FPU operation | Before pipelining | Current FPU phases |
|---|---:|---:|
| add/sub/simple unary | 10 | 6 |
| multiply/multiply-scalar/dot | 14 | 7 |

## Revision 0.7 wide vector register file (historical)

The revision-0.7 FPR reorganized from 64 lane words to sixteen 64-bit vectors
with per-lane write enables. Both asynchronous read ports return a whole vec4, so pure
data-movement instructions no longer serialize lanes through the single ALU
port schedule: `FMOV`/`FLOAD` commit one wide write at dispatch, `FPACK4`
reads two source vectors per cycle through both ports, `FUNPACK4` and
`FTRANSPOSE4` snapshot their sources and then commit one wide write per
destination row, `FSINCOS` lands both ROM results in one write, and `FIMPORT4`
commits its assembled buffer with one wide write after the fourth beat.

| FPU operation | Lane-serial phases | Current FPU phases |
|---|---:|---:|
| load/move | 6 | 2 |
| pack4 | 10 | 5 |
| unpack4 | 22 | 6 |
| transpose4 | 20 | 8 |
| sincos | 15 | 9 |
| import4 (minimum) | 14 | 10 |

The lane ALU and multiply pipeline keep their serial schedules. The SINCOS ROM
sequence is the exception: its two independent DPB ports fetch sine and cosine
in parallel. Per-lane write enables preserve the in-place aliasing guarantees
of the remaining serial lane loop.

The wide buses cost routing slack around the scalar register file: the display
system's logic-clock characterization boundary moved from 64.75 MHz to 63 MHz
(63 passes with +0.074 ns slack; 63.5 fails on three integer writeback
endpoints). The 60 MHz operating constraint still passes comfortably, so the
trade is cycle-count savings on data movement for characterization headroom
that the fitted 54 MHz clock never uses.

These counts retain blocking FPU retirement. They improve intra-instruction lane
throughput; an FPU operation remains a Stage 12 frontend barrier and does not
overlap execution of another CPU or FPU instruction.

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
   FPU instruction. The wide asynchronous reads then keep the whole vector
   available for the entire operation, so no per-lane address scheduling or
   state mux ever sits in front of RAM16.
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
