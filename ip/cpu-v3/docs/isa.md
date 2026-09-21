# CPU V3 ISA specification

The `ip/cpu-v3` crate is the executable source of truth for the ISA replacing
cpu_v2 v2.6. The exploratory `design_model/CPU_ISA.md` revision 0.2 supplied the
base encoding. This repository owns later revisions so builds never depend on a
machine-local project.

This document is self-contained: the architectural boundary, the complete
instruction encoding reference, the FPU contract, and the fault rules are all
specified below, followed by the per-revision change history. [`isa.html`](isa.html)
is a visual rendering of the same encoding for quick reference. Current hardware and fitted-system policy are documented separately
under `ip/cpu-v3/docs` and `systems/cpu-v3-tang-nano-20k/docs`.

## Architectural boundary

- Instructions and data use 16-bit word offsets within boot-selected code and
  data segments. A physical word address is the direct concatenation
  `{segment[15:0], offset[15:0]}`; segment arithmetic is never added to the
  offset and offset wrap never advances a segment.
- There are sixteen writable 16-bit GPRs and no persistent architectural flags.
  The only cross-instruction execution state besides the `PFX12` prefix is the
  transient pending test result: a three-way ordering (Less/Equal/Greater) set
  only by CMP-class instructions, consumed by the next conditional branch or
  conditional move, and expired by any other retired non-prefix instruction.
  Prefixes are transparent to it, reset leaves none, and a conditional branch
  or conditional move without one faults.
- `r0..r1` return values, `r2..r7` arguments, `r8..r11` callee-saved values,
  `r12` compiler scratch, `r13` stack pointer, and `r14` the architecturally
  fixed link register. `r15` is an ordinary allocatable register.
- The FPU v2 file is `F0..F63`, one signed Q16.16 value each (16 fractional
  bits, range about `[-32768, +32767.99998]`, all arithmetic wrapping). There
  is no `F0 = 0` and no vec2/vec3/vec4 architectural register type: a vector
  instruction views a consecutive register range. The compiler ABI reserves
  `F0..F3` for returns, places arguments compactly from `F4` (up to `F27`),
  allocates `F28..F62`, and keeps `F63` as the parallel-move scratch. Every F
  register is caller-saved and the 64-bit Q32.32 accumulator is
  caller-clobbered.
- `CSEG` supplies the high physical bits for instruction fetch. `DSEG` supplies
  them for every ordinary load and store, including stack accesses and offsets
  `0xff00..0xffff`.
- Reset establishes `CSEG = 0`, `DSEG = 0`, and `PC = 0`. Normal applications
  do not change either segment. Stage0 writes `DSEG` immediately before an
  atomic segmented jump establishes the application `CSEG` and entry offset.
- `PFX12` and an eligible adjacent consumer form one precise two-word
  operation. A consumer fault reports the prefix address and retires neither
  word. A non-consumer expires and separately retires a pending prefix.
- Every unused instruction field has the canonical value `0`; a non-canonical
  field is an invalid encoding. All reserved function slots are invalid in
  this revision, with no promise that they stay invalid forever.
- `SIGNAL rs, 0` halts execution and latches `rs` as the architectural halt
  signal at the retirement edge. `SIGNAL` types 1..15 are simulator-side
  events that retire as a NOP in hardware.

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


## Instruction encoding reference

Every physical instruction is one 16-bit word, shown as four hexadecimal nibbles
`[n3 n2 n1 n0]` with bit 15 the most significant. The major opcode is `n3`; later fields are
registers, sub-operations, or immediate bits depending on the family. Every field
not used by an instruction is canonically `0`; a non-canonical field value makes
the word an invalid encoding.

| Form | Layout | Meaning |
| --- | --- | --- |
| RRR | `opcode · rd · ra · rb` | Three-register ALU operation |
| SHM | `2 · fn · rd · operand` | Destructive shift or multiply on `rd` |
| EXT | `6 · fn · a · b` | Move, unary, register comparison, signal, special registers |
| DEV | `7 · dir/dev · ch · reg` | Device channel receive/send |
| MEM | `opcode · rd/rs · base · imm4` | Word load/store at `r[base] + imm` |
| IMM | `A · fn · rd · imm4` | In-place immediate operation on `rd` |
| BR | `B · fn · ...` | Conditional branch, relative jump, conditional move, register jump |
| PFX12 | `F · payload12` | Neutral prefix for the immediately following eligible consumer |

### Major opcode map

| n3 | Family | Form | Summary |
| --- | --- | --- | --- |
| 0/1/3/4/5 | Register ALU | `op rd ra rb` | Add, subtract, and three-operand logic |
| 2 | Shift / multiply | `2 fn rd operand` | Destructive shifts and unsigned multiplies |
| 6 | Extended / system | `6 fn a b` | Move, unary, bit queries, comparisons, SIGNAL, special registers, JSEG |
| 7 | Device | `7 dir/dev ch reg` | Single-cycle device channel receive/send |
| 8 | `LOAD` | `8 rd base imm4` | Read one 16-bit word |
| 9 | `STORE` | `9 rs base imm4` | Write one 16-bit word |
| A | Immediate | `A fn rd imm4` | Arithmetic, logic, constant construction, and immediate comparisons |
| B | Branch / move / jump | `B fn ...` | Six conditional branches, JREL/JALREL, six conditional moves, JREG/JALR |
| C | FPU VECTOR | `C Fa Fb` + word1 | Two-word vector ALU, multiply, and dot operations |
| D | FPU SCALAR | `D Fa Fb` + word1 | Two-word scalar ALU, compare, and special functions |
| E | FPU AUX | `E X Fa kind` + word1 | Two-word integer bridges and FLD/FST/FLDV/FSTV |
| F | `PFX12` | `F payload12` | Creates a pending 12-bit prefix payload |

### Register ALU (opcodes 0, 1, 3, 4, 5)

| Mnemonic | Encoding | Semantics |
| --- | --- | --- |
| `ADD rd, ra, rb` | `0 rd ra rb` | `rd = ra + rb` (wrapping) |
| `SUB rd, ra, rb` | `1 rd ra rb` | `rd = ra - rb` (wrapping) |
| `AND` / `OR` / `XOR` | `3/4/5 rd ra rb` | bitwise |

### Shift and multiply (opcode 2)

All operations in this family are destructive two-operand forms: `rd` is both
the left input and the destination. Functions 3, 7, B, and D..F are reserved
and invalid.

| fn | Mnemonic | Semantics |
| --- | --- | --- |
| 0 | `SHL rd, rs` | `rd = rd << (rs & 15)` |
| 1 | `SHR rd, rs` | `rd = rd >> (rs & 15)` (logical) |
| 2 | `ASR rd, rs` | `rd = signed(rd) >> (rs & 15)` (arithmetic) |
| 4 | `SHLI rd, imm4` | `rd = rd << imm4` |
| 5 | `SHRI rd, imm4` | `rd = rd >> imm4` (logical) |
| 6 | `ASRI rd, imm4` | `rd = signed(rd) >> imm4` (arithmetic) |
| 8/9/A | `MUL0` / `MUL8` / `MUL16 rd, rs` | unsigned `product = rd * rs` (32-bit); `rd = (product >> S) & 0xffff` for `S` in {0, 8, 16} |
| C | `MULI rd, imm` | unsigned `rd = (rd * imm) & 0xffff`; unprefixed `u4`, full `u16` with `PFX12` |

Immediate shifts never consume `PFX12`; `MULI` is the family's only prefix
consumer. There is no signed integer multiply, rounding, saturation, or
dual-register product writeback; general numeric computation belongs to the
FPU.

### Extended and system (opcode 6)

No instruction in this family consumes `PFX12`. Function 7 is reserved and
invalid.

| fn | Mnemonic | Semantics |
| --- | --- | --- |
| 0 | `MOV rd, rs` | `rd = rs` (`NOP = 6000`) |
| 1 | `NOT rd, rs` | `rd = ~rs` |
| 2 | `NEG rd, rs` | `rd = 0 - rs` |
| 3 | `SEXTB rd, rs` | `rd = sext8(rs[7:0])` |
| 4 | `CLZ rd, rs` | count leading zeros |
| 5 | `POPCNT rd, rs` | population count |
| 6 | `SEQ rd, rs` | `rd = (rd == rs) ? 1 : 0` |
| 8/9 | `SLT` / `SLTU rd, rs` | `rd = rd < rs` as 0 or 1, signed / unsigned |
| A/B | `CMPS` / `CMPU ra, rb` | pending = ordering of `ra` vs `rb`, signed / unsigned; writes no register |
| C | `SIGNAL rs, type4` | type 0 halts and latches `rs`; types 1..15 are simulator-side events that retire as a NOP in hardware |
| D | `MFSR rd, sr` | read `CSEG` (`sr=0`) or `DSEG` (`sr=1`); other selectors are invalid |
| E | `MTSR DSEG, rs` | set the data segment; other selectors are invalid |
| F | `JSEG seg, target` | atomically `CSEG = r[seg]`, `PC = r[target]` |

### Device access (opcode 7)

| Mnemonic | Encoding | Semantics |
| --- | --- | --- |
| `DEVRECV rd, dev, ch` | `7 {0,dev} ch rd` | `rd = device[dev].read(ch)` |
| `DEVSEND rs, dev, ch` | `7 {1,dev} ch rs` | `device[dev].write(ch, rs)` |

Devices 0..7 each have channels 0..15. Only `DEVRECV`/`DEVSEND` reach devices;
`LOAD`/`STORE` always reach memory. Reads sample combinational device data, writes pulse for
one execute cycle, and an unconnected device reads as zero and ignores writes. Device
instructions carry no immediate and never consume a prefix.

### Memory (opcodes 8 and 9)

| Mnemonic | Encoding | Semantics | Prefix |
| --- | --- | --- | --- |
| `LOAD rd, [base + off]` | `8 rd base imm4` | `rd = memory[base + sext4(imm4)]` | with PFX12, `off = {payload12, imm4}` |
| `STORE rs, [base + off]` | `9 rs base imm4` | `memory[base + sext4(imm4)] = rs` | same |

Address arithmetic wraps at 16 bits. Every final offset is ordinary memory in `DSEG`.

### Immediate (opcode A)

All defined A-family operations except `LDC`/`ADDC` may consume `PFX12`.
Functions E and F are reserved and invalid.

| fn | Mnemonic | Semantics | Immediate and prefix behavior |
| --- | --- | --- | --- |
| 0 | `ADDI` | `rd = rd + imm` | unsigned u4; prefix eligible (with prefix, adds the full 16-bit pattern) |
| 1 | `SUBI` | `rd = rd - imm` | unsigned u4; prefix eligible (with prefix, subtracts the full 16-bit pattern) |
| 2 | `LDI` | `rd = sext4(i4)` | with prefix, loads the full 16-bit pattern |
| 3 | `LDUI` | `rd = zext4(u4)` | with prefix, loads the full 16-bit pattern |
| 4 | `ANDI` | `rd = rd & imm` | unsigned u4; prefix eligible |
| 5 | `ORI` | `rd = rd \| imm` | unsigned u4; prefix eligible |
| 6 | `XORI` | `rd = rd ^ imm` | unsigned u4; prefix eligible |
| 7 | `LDC rd, k4` | `rd = CONST[k4]` | constant-table index; never consumes a prefix |
| 8 | `SEQI` | `rd = (rd == imm) ? 1 : 0` | signed i4 so small negative constants stay compact; prefix eligible |
| 9 | `SLTI` | `rd = signed(rd) < signed(imm)` | signed i4; prefix eligible |
| A | `SLTUI` | `rd = unsigned(rd) < unsigned(imm)` | unsigned u4; prefix eligible |
| B | `ADDC rd, k4` | `rd = rd + CONST[k4]` (wrapping) | constant-table index; never consumes a prefix |
| C | `CMPSI` | `pending = signed ordering of rd vs imm` | signed i4; prefix eligible |
| D | `CMPUI` | `pending = unsigned ordering of rd vs imm` | unsigned u4; prefix eligible |

`S*` names write a Boolean `0`/`1` to a GPR; `CMP*` names write the transient
pending test and no register.

`LDC` and `ADDC` share one symmetric 16-entry constant table `CONST`, indexed
by the immediate nibble. With `MAG = [8, 16, 24, 32, 64, 128, 256, 512]`,
indices 0..7 hold `MAG[k]` and indices 8..15 hold `-MAG[k - 8]` (two's
complement), so `CONST[k + 8] == -CONST[k]`. The entries, shown signed, are:

| k4 | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | A | B | C | D | E | F |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `CONST[k4]` | 8 | 16 | 24 | 32 | 64 | 128 | 256 | 512 | -8 | -16 | -24 | -32 | -64 | -128 | -256 | -512 |

`ADDI`/`LDI`/`LDUI` cover `0..=15` upward and `SUBI` covers the downward
side, so the table starts just past that range; the values target struct
sizes, pointer strides, and small stack-frame offsets. A `PFX12` before
`LDC`/`ADDC` simply expires unused, exactly as before any other
non-consumer.

### Branch, conditional move, and jump (opcode B)

| fn | Mnemonic | Form |
| --- | --- | --- |
| 0..5 | `BEQ/BNE/BLT/BGE/BGT/BLE off` | signed relative offset |
| 6 | `JREL off` | unconditional relative jump, no link |
| 7 | `JALREL off` | unconditional relative jump; `r14 = PC_after` |
| 8..D | `MOVEQ/MOVNE/MOVLT/MOVGE/MOVGT/MOVLE rd, rs` | `rd = rs` when the condition holds |
| E | `JREG target` | `PC = r[target]`; canonical encoding `B E 0 target` |
| F | `JALR target` | `r14 = PC_after; PC = r[target]`; canonical encoding `B F E target` |

The six condition codes appear in the same order in both halves (branch fn 0..5
and move fn 8..D). The relative offset is a signed i8 relative to the
already-incremented PC (±128 words); with `PFX12` it widens to
`off16 = {payload12[7:0], imm8}`, and `payload12[11:8]` is ignored. Conditional
branches and conditional moves consume the pending test whether or not the
branch is taken or the move writes, and fault `InvalidInstruction` without
one. `JALREL` and `JALR` link to the architecturally fixed `r14`; the middle
nibble of `JREG`/`JALR` must hold its canonical value (0 / E) and any other
value is invalid. Register forms never consume a prefix.

### FPU v2 (opcodes C, D, E)

Every FPU v2 instruction is **32 bits wide, fetched as two consecutive 16-bit
words**, and never consumes a `PFX12` prefix. Word0 exposes the source bases so
the register-file reads can start before word1 is decoded; word1 carries the
destination and the operation. The architecture encoding lives in
`architecture/encoding.rs` and is the single source of truth; the RTL extractor
is locked to it by an exhaustive test.

```text
VECTOR/SCALAR word0:  [15:12]=opcode  [11:6]=Fa  [5:0]=Fb
VECTOR word1:         [15:10]=Fd  [9:8]=len  [7:3]=subop  [2:0]=mode
SCALAR/AUX word1:     [15:10]=Fd  [9:4]=subop  [3:0]=mode
AUX word0:            [15:12]=0xE  [11:8]=X  [7:2]=Fa  [1:0]=kind
```

A vector is a consecutive register-range view, not an architectural type:
`VADD.3 F20, F4, F8` computes `F20=F4+F8`, `F21=F5+F9`, `F22=F6+F10`. Length
`00/01/10` selects vec2/vec3/vec4 and `11` is reserved. Destination/source
ranges must be exactly base-equal or fully disjoint; partial overlap is a
software contract violation and is not checked by hardware.

`0xC` VECTOR subops: `00` VADD, `01` VSUB, `02` VMUL (`q16(A*B)`), `03` VMULS
(`q16(A*B0)`), `04` VMIN, `05` VMAX, `06` VABS, `07` VNEG, `08` VFLOOR, `09`
VCEIL, `0A` VROUND, `0B` VTRUNC, `0C` VMOV, `0D` DOT, `0E` DOTADD, `0F`
DOTSTORE. `10..1F` are reserved. `mode[1:0]` is the second-source stride for the
DOT family (`00` +1, `01` +3, `10` +4; `11` reserved) and is reserved for the
other vector subops. `DOT` sets `ACC = sum(A[i]*B[i])`, `DOTADD` accumulates,
and `DOTSTORE` narrows the completed sum to `Fd` and clears ACC.

`0xD` SCALAR subops: `00` ADD, `01` SUB, `02` MUL, `03` MIN, `04` MAX, `05`
ABS, `06` NEG, `07` FLOOR, `08` CEIL, `09` ROUND, `0A` TRUNC, `0B` CMP, `0C`
RCP, `0D` RSQRT, `0E` SINCOS, `0F` MOV. `10..3F` are reserved. `CMP` writes the
transient pending test (signed ordering of `Fa` and `Fb`), so conditional
branches and conditional moves consume it exactly as they consume
`CMPS`/`CMPU`. `SINCOS` selects its result with `mode[1:0]`: `00` writes
`Fd=sin`, `Fd+1=cos` (requires `Fd<=62`), `01` writes `sin`, `10` writes `cos`,
`11` is reserved; `mode[3:2]` is reserved and must be zero. Every other scalar
subop has a reserved mode field that must be zero.

`0xE` AUX word0 `kind` is `00` integer-register index, `01` constant-table
index, `10` selector, `11` reserved. Only kind `00` is defined today; word1
subops are `00` FLD, `01` FST, `02` ILO2F, `03` IHI2F, `04` FLO2I, `05` FHI2I,
`06` I16TOF, `07` FTOI16, and `08..3F` are reserved. FLD/FST use `mode[1:0] + 1`
lanes (`00` scalar through `11` vec4) with `mode[3:2]` reserved. `FLD` loads
`Fd+i = {mem[a+2i+1], mem[a+2i]}` and `FST` stores the same two halves, **low
half first**, at `a = {DSEG, GPR[X]}`. `ILO2F`/`IHI2F` copy `GPR[X]` into the
low/high half of `Fd`; `FLO2I`/`FHI2I` copy a half back; `I16TOF` sign-extends
`GPR[X]` into a Q16.16 value; `FTOI16` truncates `Fa` toward zero into `GPR[X]`.

All F registers are signed Q16.16 and every operation **wraps**: there is no
saturation and no exception output. `FLOOR` clears the 16 fraction bits;
`CEIL` adds one unit when a fraction remains; `ROUND` is round-half-up
(`floor(x + 0x8000)`, ties toward `+infinity`); `TRUNC` is toward zero. The
wide accumulator is signed 64-bit Q32.32. Every Q16.16 product fits exactly,
but accumulation keeps only the low 64 bits: extreme legal DOT operands can
wrap the ACC, and `DOTSTORE` takes `ACC[47:16]` from that wrapped sum. `RCP(0)`
returns `0x7FFF_FFFF` (sign applied for negative magnitudes) and `RSQRT(x)` is
zero for `x <= 0`; both are approximate special functions with no IEEE special
values. The hidden register-file BSRAM region holds the RCP/RSQRT/SINCOS
lookup tables.

The prescale library family (`v3_length2_shift` -> `u16`,
`v3_length2_scaled` -> `fix16`, `v3_normalize_safe`, `v3_distance_gt`) is a
compiler library, not a new opcode; its reference model and `2^-k` scaling rule
live in the CPU V3 architecture crate, and the compiler lowers it to existing
FPU v2 instructions. `v3_length2_scaled` returns the **prescaled** squared
length `|v|^2 * 2^-2k` (not an unscaled length), and `v3_length2_shift` returns
`k`. `scaled << 2k` is an original-scale approximation, not an exact
reconstruction, because shifting the components and narrowing the dot result
discard low bits when `k > 0`. `v3_distance_gt` is an approximate **ordinary**
distance comparison `|a - b| > threshold`: both vectors are prescaled by one `k`
(with one extra bit of headroom) before the subtraction, the scaled squared
length is narrowed once through `DOTSTORE`, the ordinary distance is
approximated as `s * rsqrt(s)`, and the ordinary Q16.16 threshold is shifted by
`k` before the scalar `CMP`. The shifts, the single `DOTSTORE` narrowing, and
the `RSQRT`/`MUL` approximation make the boundary approximate; a negative
threshold is always exceeded by the non-negative distance.

### PFX12 and wide operations

`PFX12 0xabc` (renamed from `IMMHI12`) supplies a neutral 12-bit payload to the
immediately following eligible consumer; the prefix itself does not imply
whether the payload occupies the high or low part of the consumer's effective
value. For example `PFX12 0xabc; LDUI r3, 0xd` loads `r3 = 0xabcd`; the pair
retires two physical words together. The closed consumer set is `LOAD`,
`STORE`, `MULI`, every defined major-A operation except `LDC`/`ADDC`, and the
major-B relative forms (functions 0..7). Each consumer family defines the
composition:

```text
integer imm4 consumer:  value16  = {payload12, imm4}
relative off8 consumer: offset16 = {payload12[7:0], imm8}
```

Relative consumers use only `payload12[7:0]`; `payload12[11:8]` is ignored and
carries no canonical-value fault. Register ALU, shift/multiply register and
shift-immediate forms, `LDC`/`ADDC`, the major-6 family, device instructions,
conditional moves, register jumps, the FPU, reserved encodings, and another
prefix do not consume a prefix. A prefix is transparent to the pending test result. A
non-consumer expires a pending prefix and retires it separately; a second
prefix replaces the first; if a prefixed consumer faults, the reported address
is the prefix address and neither word retires.

### Fault and retirement summary

| Condition | Result |
| --- | --- |
| Reserved or malformed encoding (reserved subop/length/mode, non-canonical fields, a truncated FPU pair) | `InvalidInstruction` (code 1); the instruction does not retire |
| Conditional branch or conditional move without a pending test | `InvalidInstruction`; does not retire |
| Address outside fitted physical memory (including FLD/FST/FLDV/FSTV) | physical-address fault at the faulting offset; does not retire |
| `SIGNAL` type 0 after halt | re-reports the latched halt signal |

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

## Revision 0.7 (historical)

Revision 0.7 introduced a blocking fix16 FPU in the `D fn a b` family: sixteen
F registers of four signed Q8.8 lanes and a saturating 40-bit accumulator.
That architecture is **retired**; it is replaced wholesale by the two-word
Q16.16 FPU v2 above. The old single-word encodings, Q8.8 semantics, saturating
ACC, `FIMPORT4`/`FEXPORT4`, `FACCLOAD`/`FACCSTORE`, and `FUNARY` subops no
longer exist, and a revision-0.7 FPU word is no longer a valid instruction.
The revision-0.7 knowledge and RTL remain only under the project history store.

## Revision 0.8

Revision 0.8 is a breaking integer rearrangement; no binary compatibility with
earlier revisions is preserved. The compiler, simulators, RTL, debugger
decoding, and generated boot images switch at the same boundary. At the time of
revision 0.8 the fix16 FPU family at major `D` was unchanged and majors `C`/`E`
were reserved. **The FPU was subsequently replaced wholesale by the two-word
Q16.16 FPU v2** documented above: majors `C`, `D`, and `E` now start a 32-bit
FPU pair, and the old single-word Q8.8 `Dxxx` encoding is no longer valid.

- Majors 0/1/3/4/5 keep the three-register `ADD`/`SUB`/`AND`/`OR`/`XOR`. The
  old three-register `MUL` (major 2) and register-count `SHL`/`ASR` (majors
  6/7) move into the new destructive shift/multiply family at major 2, which
  adds the logical-right shift `SHR`, the immediate shifts `SHLI`/`SHRI`/`ASRI`
  (moved from major A), the unsigned product windows `MUL8`/`MUL16`, and the
  unsigned `MULI` (moved from major A, now shift-0 only and bit-pattern
  unsigned).
- Major 6 becomes the extended/system family: `MOV`/`NOT`/`NEG`/`SEXTB`/`CLZ`/
  `POPCNT`, the new Boolean-producing `SEQ`, the destructive `SLT`/`SLTU`, the
  pending-test `CMPS`/`CMPU`, the new `SIGNAL`, and `MFSR`/`MTSR`/`JSEG`.
  `NOP` is now `6000` (`MOV r0, r0`).
- `SIGNAL rs, type4` replaces `HALT`: type 0 halts and latches `rs` at the
  retirement edge (so `HALT` is `SIGNAL r0, 0` = `6C00`), and types 1..15 are
  simulator-side events that retire as a NOP in hardware.
- Device access moves from major C to major 7 with an unchanged field layout.
- Major A keeps only immediate arithmetic/logic, constant construction
  (`LDI`/`LDUI` move to functions 2/3), and comparisons. `CMPEQI` is renamed
  `SEQI` so `S*` consistently means a Boolean register result while `CMP*`
  means a pending-test result. The shift-immediates and `MULI` move to major
  2. Functions 7 and B are `LDC`/`ADDC`, indexing a shared symmetric 16-entry
  constant table (`MAG = [8, 16, 24, 32, 64, 128, 256, 512]`; indices 0..7
  hold `MAG[k]`, indices 8..15 hold `-MAG[k - 8]`) that starts just past the
  `0..=15` range `ADDI`/`LDI`/`LDUI` already cover; they never consume
  `PFX12`. Functions E and F are reserved.
- Amendment (still revision 0.8): `ADDI`/`SUBI` read the unprefixed immediate
  as an unsigned u4 (`0..=15`) instead of a signed i4 — negative adjustments
  are `SUBI`'s job, so the signed range wasted half the encodings. The
  `PFX12`-widened forms are unchanged: both add/subtract the full 16-bit
  immediate pattern.
- Major B becomes the symmetric control family: six conditional branches
  (0..5), `JREL` (6), `JALREL` (7), six conditional moves `MOVEQ`..`MOVLE`
  (8..D), `JREG` (`B E 0 target`), and `JALR` (`B F E target`). Conditional
  moves consume the pending test exactly like conditional branches, including
  the missing-test fault. `JALREL`/`JALR` link to the fixed `r14`; the
  `JREG`/`JALR` middle nibble must hold its canonical value.
- `IMMHI12` is renamed `PFX12` and carries a neutral 12-bit payload. The
  closed consumer set is `LOAD`/`STORE`, `MULI`, every defined major-A
  operation except `LDC`/`ADDC`, and the major-B relative forms 0..7.
  Relative consumers use only `payload12[7:0]`; `payload12[11:8]` is ignored
  without a canonical-value fault.
- All unused fields are canonically `0`; any non-canonical field value is an
  invalid encoding. All reserved function slots are invalid in this revision.
