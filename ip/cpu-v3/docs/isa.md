# CPU V3 migration specification

The `ip/cpu-v3` crate is the executable source of truth for the ISA replacing
cpu_v2 v2.6. The exploratory `design_model/CPU_ISA.md` revision 0.2 supplied the
base encoding. This repository owns later revisions so builds never depend on a
machine-local project.

The self-contained visual reference is [`isa.html`](isa.html). It documents the
complete encoding alongside architectural behavior, addressing, faults, and the
compiler ABI. Current hardware and fitted-system policy are documented separately
under `ip/cpu-v3/docs` and `systems/cpu-v3-tang-nano-20k/docs`.

## Architectural boundary

- Instructions and data use 16-bit word offsets within boot-selected code and
  data segments. A physical word address is the direct concatenation
  `{segment[15:0], offset[15:0]}`; segment arithmetic is never added to the
  offset and offset wrap never advances a segment.
- There are sixteen writable 16-bit GPRs and no persistent architectural flags.
  The only cross-instruction execution state besides the `IMMHI12` prefix is the
  transient pending test result: a three-way ordering (Less/Equal/Greater) set
  only by CMP-class instructions, consumed by the next conditional branch, and
  expired by any other retired non-prefix instruction. Prefixes are transparent
  to it, reset leaves none, and a conditional branch without one faults.
- `r0..r1` return values, `r2..r7` arguments, `r8..r11` callee-saved values,
  `r12` compiler scratch, `r13` stack pointer, and `r14` the architecturally
  fixed link register. `r15` is an ordinary allocatable register.
- `CSEG` supplies the high physical bits for instruction fetch. `DSEG` supplies
  them for every ordinary load and store, including stack accesses and offsets
  `0xff00..0xffff`.
- Reset establishes `CSEG = 0`, `DSEG = 0`, and `PC = 0`. Normal applications
  do not change either segment. Stage0 writes `DSEG` immediately before an
  atomic segmented jump establishes the application `CSEG` and entry offset.
- `IMMHI12` and an eligible adjacent consumer form one precise two-word
  operation. A consumer fault reports the prefix address and retires neither
  word. A non-consumer expires and separately retires a pending prefix.
- `HALT` carries no result field. Its architectural halt signal is the value of
  `r0` latched at the HALT retirement edge.

The baseline offset map inside the selected data segment is:

| Range | Initial use |
| --- | --- |
| `0x0000..` | linked code, growing upward |
| `0x4000..` | static data |
| `0x8000..` | heap baseline |
| below `0x10000` | stack, growing downward from the exclusive segment top |

`CompilerOptions::default()` selects these boundaries. A zero initial stack
pointer denotes the exclusive segment top `0x10000`; the first allocation
therefore wraps naturally into offset `0xffff`.

## Revision 0.3

Compiler lowering showed that revision 0.2 could not implement general signed
or unsigned register comparisons correctly. Testing the sign of `a - b` fails
at overflow boundaries. Revision 0.3 assigns previously reserved operations:

| Encoding | Name | Operation |
| --- | --- | --- |
| `A B rd imm4` | `SLTUI` | `rd = unsigned(rd) < unsigned(imm)` |
| `E 9 rd rs` | `SLT` | `rd = signed(rd) < signed(rs)` |
| `E A rd rs` | `SLTU` | `rd = unsigned(rd) < unsigned(rs)` |
| `E B rd rs` | `POPCNT` | `rd = popcount(rs)` |

`SLTUI` accepts `IMMHI12`. These additions do not consume the reserved `0xDxxx`
FPU space. Equality still uses `CMPEQI` for immediates or `XOR` plus `BZ/BNZ`
for registers. `POPCNT` preserves the existing rcc `cnt1` intrinsic without a
large software sequence.

## Revision 0.4

Revision 0.4 makes the complete fitted physical memory addressable without widening
ordinary pointers or changing compiler-generated load/store instructions.

| Encoding | Name | Operation |
| --- | --- | --- |
| `E C rd sr` | `MFSR` | read `CSEG` (`sr=0`) or `DSEG` (`sr=1`) |
| `E D 1 rs` | `MTSR DSEG` | set the boot-time data segment from `rs` |
| `E E seg target` | `JSEG` | atomically set `CSEG = r[seg]`, `PC = r[target]` |
| `E F ..` | reserved | invalid instruction |

Directly writing `CSEG` is deliberately impossible because fetching a
sequential instruction after such a write would be pipeline-dependent.
`JSEG` is the only segmented transfer in the initial ABI. Functions and
function pointers remain near and within one code segment; dynamic data-bank
switching and far calls are outside the compiler contract.

The compiler continues to emit 16-bit offsets. Its linked code must fit one
64K-word code window, and static data, heap, and stack must fit one 64K-word
data window. `CompilerOptions::code_base` (CLI `--target cpu-v3 --code-base`) relocates
the linked code offsets without adding padding to the output file. The offline
packer places those bytes at the matching physical segment and offset.

## Revision 0.5

Revision 0.5 replaces the test-register branches with a transient pending test
result, moves relative jumps into the B family, turns opcode C into device
instructions, and frees `r15` for general allocation.

| Encoding | Name | Operation |
| --- | --- | --- |
| `B cond imm8` | `BEQ/BNE/BLT/BGE/BGT/BLE` | branch on the pending test result (cond 0..5) |
| `B 8 imm8` | `JREL` | unconditional relative jump, no link |
| `B 9 imm8` | `JALREL` | unconditional relative jump; link fixed to `r14` |
| `C {0,dev} ch rd` | `DEVRECV` | `rd = device[dev].read(ch)` |
| `C {1,dev} ch rs` | `DEVSEND` | `device[dev].write(ch, rs)` |
| `E B rd rs` | `CMPS` | pending = signed ordering of `rd` vs `rs`; writes no register |
| `E C rd rs` | `CMPU` | pending = unsigned ordering of `rd` vs `rs`; writes no register |
| `A C rd imm4` | `CMPSI` | pending = signed ordering vs immediate (sext4, prefix eligible) |
| `A D rd imm4` | `CMPUI` | pending = unsigned ordering vs immediate (zext4, prefix eligible) |

Motivation and rules:

- The pending test result keeps the "no architectural flags" spirit as an
  `IMMHI12`-style transient rather than a persistent flag: only CMP-class
  instructions set it, only conditional branches consume it, any other retired
  non-prefix instruction expires it, and prefixes are transparent to it. A
  conditional branch with no pending result faults `InvalidInstruction`
  (reported at the prefix address when prefixed), which turns a forgotten or
  misplaced compare into an immediate failure instead of a data-dependent
  branch on a stale register. `CMPSI r, 0` covers the old branch-on-value uses.
- B-family conditions now take a signed 8-bit offset (±128 words) instead of a
  test register plus imm4. The old `B cond test imm4` conditions
  (`BZ/BNZ/BN/BNN/BP/BNP/BODD/BEVEN`) are removed, and conditions 6, 7, and
  A..F are reserved as `InvalidInstruction`. The prefixed wide-offset rule
  `off16 = {prefix[7:0], imm8}` generalizes from the old C family to all
  B-family consumers.
- The old opcode-C `JREL`/`JALREL` are removed; the jumps live at B-family
  conditions 8 and 9. The link register is architecturally fixed to `r14`:
  `JALREL` has no link field, and `JALR` (`E 5`) faults unless its link field
  encodes 14.
- Opcode C becomes single-word device instructions over devices 0..7 and
  channels 0..15. Device instructions carry no immediate and never consume a
  prefix. The `dev_send`/`dev_recv` compiler intrinsics lower to one instruction
  each.
- With no reserved device base register left, `r15` joins the allocatable and
  caller-saved sets; the prologue no longer initializes it.
- The prefix-consumer set is closed: `LOAD`/`STORE`, all A-family functions
  except the shifts (fn 5..=7), and B-family conditions 0..5, 8, and 9. The
  C-family device instructions do not consume a prefix.
- The E family is rearranged so the four comparisons sit together:
  `SLT`/`SLTU` keep 9/A, `CMPS`/`CMPU` take B/C, `POPCNT` moves B→0,
  `MFSR`/`MTSR`/`JSEG` move C/D/E→D/E/F. All sixteen E slots are now
  occupied. The A family is unchanged; the unsigned immediate compare is
  spelled `CMPUI` to match `CMPSI`.

## Revision 0.6

Revision 0.6 removes address-mapped device access. `LOAD` and `STORE` always
form `{DSEG, offset}` and the complete 16-bit offset range is ordinary memory.
Only `DEVRECV` and `DEVSEND` can access a device. The core exposes their decoded
3-bit device index, 4-bit channel, direction, and 16-bit data on a dedicated
single-cycle port; reads are combinational and writes pulse for one execute
cycle. An unconnected device reads as zero and ignores writes. The three-bit
encoding permanently limits the architectural device space to eight devices.

With the high offset page restored to memory, `SP = 0` again denotes the
exclusive `0x10000` top of a 64K-word data segment. This is the default compiler
and boot ABI stack value.

## Revision 0.7

Revision 0.7 assigns the complete `D fn a b` family to the blocking fix16 FPU.
It adds sixteen four-lane F registers, each lane holding signed Q8.8 data, and
a signed saturating 40-bit accumulator. An FPU instruction is a core-execution
barrier: it completes and retires before the core accepts its successor for
execution, although the fitted system's independent fetch queue may fetch ahead.
FPU instructions never consume `IMMHI12`.

| fn | Name | Operation |
| --- | --- | --- |
| 0/1 | `FLOAD`/`FSTORE` | raw fix16 bridge between a GPR and lane x |
| 2/3 | `FIMPORT4`/`FEXPORT4` | four aligned words at `{DSEG, rb}` |
| 4..7 | `FMOV`/`FPACK4`/`FUNPACK4`/`FTRANSPOSE4` | register reorganization |
| 8..A | `FADD`/`FSUB`/`FMUL` | saturating component arithmetic |
| B/C | `FDOT4ACC`/`FACCSTORE` | wide accumulation and rounded lane writeback |
| D | `FCMP` | signed lane-x ordering for the pending test |
| E | `FUNARY` | reciprocal, reciprocal sqrt, sin/cos, and simple unary operations |
| F | `FMULS` | vector multiplied by `Fb.x` |

All narrowing uses round-to-nearest with ties to even followed by signed Q8.8
saturation. `FRCP(0)` and `FRSQRT(x)` for `x <= 0` raise FPU-domain fault code
2 without modifying FPU state. Four-word transfers require `rb & 3 == 0`; a
misaligned transfer faults before issuing memory traffic. Architecturally, each
transfer reads or writes four consecutive words.

The three continuation bits are derived combinationally from the current four
lane values and are only an execution hint. They are not architectural state
and are neither spilled nor restored. All F registers are caller-saved. The
software ABI requires ACC to be zero at every function entry, call, and return.
