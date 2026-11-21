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

- Instructions use a 16-bit word offset in `CSEG`. Data uses a 16-bit logical
  offset split into four 16-Kword quarters. `DSEG0..DSEG3` independently map
  those quarters to aligned physical 16-Kword pages:
  `{DSEGn[7:0], offset[13:0]}`, where `n = offset[15:14]`.
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
- `CSEG` supplies the high physical bits for instruction fetch. `DSEG0..3`
  translate every ordinary data access, including stack and FPU accesses.
  The legacy `DSEG` view is `DSEG0 >> 2`; writing it atomically establishes
  four consecutive pages `{value*4 + 0, 1, 2, 3}`.
- Reset establishes `CSEG = 0`, `DSEG0..3 = {0,1,2,3}`, and `PC = 0`. Stage0
  uses the legacy bulk `DSEG` write immediately before an atomic segmented
  jump establishes the application `CSEG` and entry offset.
- `PFX12` and an eligible adjacent consumer form one precise two-word
  operation. A consumer fault reports the prefix address and retires neither
  word. A non-consumer expires and separately retires a pending prefix.
- Every unused instruction field has the canonical value `0`; a non-canonical
  field is an invalid encoding. All reserved function slots are invalid in
  this revision, with no promise that they stay invalid forever.
- `SIGNAL rs, 0` halts execution and latches `rs` as the architectural halt
  signal at the retirement edge. `SIGNAL` types 1..15 are simulator-side
  events that retire as a NOP in hardware.

## Segmented addressing and ABI memory map

All architectural addresses are **word addresses**. Instruction and data addresses use different
translations:

- Instruction fetch forms the physical word address `{CSEG, PC}`. `PC` is a 16-bit offset, so one
  CSEG value exposes 64 Kwords (128 KiB) of code. A fitted target reports an out-of-range physical
  address instead of truncating unsupported high CSEG bits.
- An ordinary data pointer remains a 16-bit logical offset. Its high two bits select one of four
  page registers and its low 14 bits select a word within that page:

  ```text
  quarter = offset[15:14]
  page    = DSEG[quarter]              // one of DSEG0..DSEG3
  physical_word_address = {page[7:0], offset[13:0]}
  ```

  Each logical quarter and physical page is therefore 16 Kwords (32 KiB). The four page registers
  are independent: pages need not be adjacent or ordered, and duplicate mappings are permitted.
  Hardware does not check for aliasing. Offset arithmetic still wraps at 16 bits before translation.

The legacy `DSEG` special-register view exists for boot and old bulk-switch code. Reading it returns
`DSEG0 >> 2`. Writing a value `s < 64` atomically establishes the consecutive mapping
`DSEG0..DSEG3 = {s*4, s*4+1, s*4+2, s*4+3}`. Individual `DSEG0..DSEG3` writes accept an 8-bit page
number and may subsequently make the legacy readback only a base-page compatibility view.

Tang Nano 20K applications use distinct code and data mappings. Linked instructions start at
offset `0x0000` of CSEG. Boot uses the legacy DSEG write to establish four consecutive physical
pages, after which libraries may remap the fourth quarter independently. The fitted compiler ABI is:

| Logical word range | Initial use | Page mapping |
| --- | --- | --- |
| `0x0000..0x1fff` | static data, growing upward (16 KiB reservation) | first half of DSEG0 |
| `0x2000..0x9ffe` | heap (`0x7fff` words, current boundary-tag limit) | remainder of DSEG0, all of DSEG1, and part of DSEG2 |
| `0x9fff..0xbfff` | stack capacity, growing downward from exclusive top `0xc000` | remainder of DSEG2 |
| `0xc000..0xffff` | dynamically remappable library/device window | all of DSEG3 |

Thus DSEG0..2 provide the ordinary 96-KiB application arena; DSEG3 provides one 32-KiB scoped
window. The ISA does not protect the heap/stack boundary and does not assign ownership to the
dynamic window. Those policies belong to the runtime and device libraries.

`CompilerOptions::for_separate_code_and_data_segments` selects these boundaries.
`CompilerOptions::default()` retains a conservative unified-memory layout for standalone images
and simulators that do not establish distinct CSEG and DSEG mappings.


## Instruction encoding reference

Each instruction word is 16 bits, shown as four hexadecimal nibbles
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

See the [FPU two-word encodings](#fpu-v2-opcodes-c-d-e) for opcodes C, D, and E.

### Major opcode map

| n3 | Family | Form | Summary |
| --- | --- | --- | --- |
| 0/1/3/4/5 | Register ALU | `op rd ra rb` | Add, subtract, and three-operand logic |
| 2 | Shift / multiply | `2 fn rd operand` | Destructive shifts and unsigned multiplies |
| 6 | Extended / system | `6 fn a b` | Move, unary, bit queries, comparisons, asynchronous line copy, SIGNAL, special registers, JSEG |
| 7 | Device | `7 dir/dev ch reg` | Single-cycle device channel receive/send |
| 8 | `LOAD` | `8 rd base imm4` | Read one 16-bit word |
| 9 | `STORE` | `9 rs base imm4` | Write one 16-bit word |
| A | Immediate | `A fn rd imm4` | Arithmetic, logic, constant construction, and immediate comparisons |
| B | Branch / move / jump | `B fn ...` | Six conditional branches, JREL/JALREL, six conditional moves, JREG/JALR |
| C | [FPU VECTOR](#fpu-vector-opcode-c) | `C Fa Fb` + word1 | Two-word vector ALU, multiply, and dot operations |
| D | [FPU SCALAR](#fpu-scalar-opcode-d) | `D Fa Fb` + word1 | Two-word scalar ALU, compare, and special functions |
| E | [FPU AUX](#fpu-aux-opcode-e) | `E X Fa kind` + word1 | Two-word integer bridges and FLD/FST/FLDV/FSTV |
| F | `PFX12` | `F payload12` | Creates a pending 12-bit prefix payload |

### Register ALU (opcodes 0, 1, 3, 4, 5)

| Mnemonic | Encoding | Semantics |
| --- | --- | --- |
| `ADD rd, ra, rb` | `0 rd ra rb` | `rd = ra + rb` (wrapping) |
| `SUB rd, ra, rb` | `1 rd ra rb` | `rd = ra - rb` (wrapping) |
| `AND` / `OR` / `XOR` | `3/4/5 rd ra rb` | bitwise |

### Shift and multiply (opcode 2)

The arithmetic operations are destructive two-operand forms: `rd` is both
the left input and destination. Function 7 instead reads two pointers without
writing a register. Functions 3, B, and D..F remain reserved and invalid.

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

Function 7 in opcode 2 is reserved and faults; no compatibility alias is provided.

### Extended and system (opcode 6)

No instruction in this family consumes `PFX12`.

| fn | Mnemonic | Semantics |
| --- | --- | --- |
| 0 | `MOV rd, rs` | `rd = rs` (`NOP = 6000`) |
| 1 | `NOT rd, rs` | `rd = ~rs` |
| 2 | `NEG rd, rs` | `rd = 0 - rs` |
| 3 | `SEXTB rd, rs` | `rd = sext8(rs[7:0])` |
| 4 | `CLZ rd, rs` | count leading zeros |
| 5 | `POPCNT rd, rs` | population count |
| 6 | `SEQ rd, rs` | `rd = (rd == rs) ? 1 : 0` |
| 7 | `LCOPY rsource, rdestination` | asynchronously copy the newest aligned source cache line to destination DRAM; both operands are DSEG-mapped word pointers |
| 8/9 | `SLT` / `SLTU rd, rs` | `rd = rd < rs` as 0 or 1, signed / unsigned |
| A/B | `CMPS` / `CMPU ra, rb` | pending = ordering of `ra` vs `rb`, signed / unsigned; writes no register |
| C | `SIGNAL rs, type4` | type 0 halts and latches `rs`; types 1..15 are simulator-side events that retire as a NOP in hardware |
| D | `MFSR rd, sr` | read `CSEG` (`sr=0`), legacy `DSEG` (`sr=1`), or `DSEG0..3` (`sr=2..5`) |
| E | `MTSR/control` | selector `a`: bulk/page mapping and asynchronous D-cache commands listed below |
| F | `JSEG seg, target` | atomically `CSEG = r[seg]`, `PC = r[target]` |

The function-E selector map is:

| a | Encoding | Operation |
| --- | --- | --- |
| 1 | `6 E 1 rs` | `MTSR DSEG, rs`: require `rs < 64`; set `DSEG0..3 = rs*4 + {0,1,2,3}` atomically |
| 2..5 | `6 E a rs` | `MTSR DSEG(a-2), rs`: require `rs < 256`; replace one physical page mapping |
| 6 | `6 E 6 raddr` | `DCLEANL raddr`: asynchronously write back a dirty resident line; a miss/clean hit is a no-op |
| 7 | reserved | invalid instruction |
| 8 | `6 E 8 0` | `DWAIT`: wait for the D-cache command engine and raise a data-memory fault if its sticky command result is an error |

`LCOPY` is encoded `6 7 source destination`. Both registers hold ordinary
16-bit word pointers and must have their low four bits zero (32-byte alignment).
Each pointer independently selects DSEG0..3 with its high two bits. Both mapped
22-bit physical addresses are latched on command acceptance; later DSEG writes
do not retarget the copy. Neither register is written. An unaligned operand
raises a non-retiring data-memory fault; both complete lines must fit memory.

The command waits for older scalar/FPU stores and D-cache acceptance, then
retires while independent instructions continue. A later D-cache access or
`DWAIT` waits for completion. A hit supplies the latest source data, including
dirty bytes; a miss uses ordinary source-set victim writeback and refill. The
source stays resident and retains its dirty bit. The complete source line is
written to destination DRAM in four 64-bit beats, without allocating a target.
A cached destination alias in its own set is invalidated even when dirty; its
obsolete bytes are not written back. Comparing complete physical rows avoids
confusing different sets that have equal tags. If both pointers map to the
same physical row, invalidation is skipped but DRAM is still really written.
Read/write failures keep the existing valid-sweep and sticky `DWAIT` error policy.

`DCLEANL` shares the issue rule and writes one dirty resident hit; a miss/clean
hit completes locally. Command admission and terminal completion are distinct.

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
| `LOAD rd, [base + off]` | `8 rd base imm4` | `rd = memory[base + zext4(imm4)]` | with PFX12, `off = {payload12, imm4}` |
| `STORE rs, [base + off]` | `9 rs base imm4` | `memory[base + zext4(imm4)] = rs` | same |

Unprefixed memory offsets are unsigned `0..15`; other offsets use `PFX12`.
The prefixed 16-bit pattern still supports negative displacements.
Address arithmetic wraps at 16 bits. The final offset selects one of `DSEG0..3`
with its high two bits and a word within that physical page with its low 14 bits.

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

Word 0 is fetched first. FPU pairs never consume `PFX12`.
`Fa`, `Fb`, and `Fd` index `F0..F63`. Subop values are hexadecimal.
Emit only defined encodings. Reserved FPU encodings raise
`InvalidInstruction` in the instruction simulator and have undefined behavior
on hardware.

F registers hold signed Q16.16 values for arithmetic. Integer bridges and
memory transfers also expose the raw bits. Ordinary arithmetic wraps without
saturation or exception flags. `q16(P)` means bits `P[47:16]` of a signed
64-bit product or accumulator, with no rounding.

#### FPU VECTOR (opcode C)

<table>
  <thead>
    <tr>
      <th scope="col">Bit</th>
      <th scope="col">15</th><th scope="col">14</th><th scope="col">13</th><th scope="col">12</th>
      <th scope="col">11</th><th scope="col">10</th><th scope="col">9</th><th scope="col">8</th>
      <th scope="col">7</th><th scope="col">6</th><th scope="col">5</th><th scope="col">4</th>
      <th scope="col">3</th><th scope="col">2</th><th scope="col">1</th><th scope="col">0</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <th scope="row">Word 0</th>
      <td colspan="4"><code>0xC</code><br><span>[15:12]</span></td>
      <td colspan="6"><code>Fa</code><br><span>[11:6]</span></td>
      <td colspan="6"><code>Fb</code><br><span>[5:0]</span></td>
    </tr>
    <tr>
      <th scope="row">Word 1</th>
      <td colspan="6"><code>Fd</code><br><span>[15:10]</span></td>
      <td colspan="2"><code>len</code><br><span>[9:8]</span></td>
      <td colspan="5"><code>subop</code><br><span>[7:3]</span></td>
      <td colspan="3"><strong>mode</strong><br><span>[2:0]</span></td>
    </tr>
  </tbody>
</table>

`len = 00/01/10` selects `N = 2/3/4` lanes. `11` is reserved. For lane `i`
from 0 to `N-1`, `A[i] = F[Fa+i]`, `B[i] = F[Fb+i]`, and `D[i] = F[Fd+i]`.
For example, `VADD.3 F20, F4, F8` writes F20 through F22 from the three
corresponding pairs. Register ranges must fit within F0 through F63.
Vector-valued destinations may alias a source at the same base or be fully
disjoint. Software must avoid partial overlap.

| Subop | Mnemonic | Semantics |
| --- | --- | --- |
| `00` | `VADD.N Fd, Fa, Fb` | `D[i] = A[i] + B[i]` |
| `01` | `VSUB.N Fd, Fa, Fb` | `D[i] = A[i] - B[i]` |
| `02` | `VMUL.N Fd, Fa, Fb` | `D[i] = q16(A[i] * B[i])` |
| `03` | `VMULS.N Fd, Fa, Fb` | `D[i] = q16(A[i] * F[Fb])` using one scalar second source |
| `04` | `VMIN.N Fd, Fa, Fb` | `D[i] = min(A[i], B[i])` |
| `05` | `VMAX.N Fd, Fa, Fb` | `D[i] = max(A[i], B[i])` |
| `06` | `VABS.N Fd, Fa` | `D[i] = abs(A[i])` |
| `07` | `VNEG.N Fd, Fa` | `D[i] = -A[i]` |
| `08..0B` | Reserved | Invalid encoding |
| `0C` | `VMOV.N Fd, Fa` | `D[i] = A[i]` |
| `0D` | `DOT.N Fa, Fb` | `ACC = sum` |
| `0E` | `DOTADD.N Fa, Fb` | `ACC = ACC + sum` |
| `0F` | `DOTSTORE.N Fd, Fa, Fb` | `F[Fd] = q16(sum)`, then clear `ACC` |
| `10..1F` | Reserved | Invalid encoding |

For DOT, DOTADD, and DOTSTORE, `sum` is the sum of `F[Fa+i] * F[Fb+i*stride]`
for all lanes. The products are exact signed 64-bit Q32.32 values. The sum
and ACC addition wrap modulo 2^64. DOTSTORE takes the result from this
instruction's sum and writes one F register. DOT and DOTADD write only ACC.

| DOT-family `mode[1:0]` | Second-source stride |
| --- | --- |
| `00` | 1 |
| `01` | 3 |
| `10` | 4 |
| `11` | Reserved |

VECTOR `mode[2]` must be zero. All three mode bits must be zero outside the
DOT family. Encode unused operands as zero, including `Fb` for unary
operations and `Fd` for DOT and DOTADD.

#### FPU SCALAR (opcode D)

<table>
  <thead>
    <tr>
      <th scope="col">Bit</th>
      <th scope="col">15</th><th scope="col">14</th><th scope="col">13</th><th scope="col">12</th>
      <th scope="col">11</th><th scope="col">10</th><th scope="col">9</th><th scope="col">8</th>
      <th scope="col">7</th><th scope="col">6</th><th scope="col">5</th><th scope="col">4</th>
      <th scope="col">3</th><th scope="col">2</th><th scope="col">1</th><th scope="col">0</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <th scope="row">Word 0</th>
      <td colspan="4"><code>0xD</code><br><span>[15:12]</span></td>
      <td colspan="6"><code>Fa</code><br><span>[11:6]</span></td>
      <td colspan="6"><code>Fb</code><br><span>[5:0]</span></td>
    </tr>
    <tr>
      <th scope="row">Word 1</th>
      <td colspan="6"><code>Fd</code><br><span>[15:10]</span></td>
      <td colspan="6"><code>subop</code><br><span>[9:4]</span></td>
      <td colspan="4"><strong>mode</strong><br><span>[3:0]</span></td>
    </tr>
  </tbody>
</table>

`mode[1:0]` selects the product window for MUL or the output for SINCOS.
`mode[3:2]` must be zero. Every other scalar subop requires `mode = 0000`.

| Subop | Mnemonic | Semantics |
| --- | --- | --- |
| `00` | `ADD Fd, Fa, Fb` | `Fd = Fa + Fb` |
| `01` | `SUB Fd, Fa, Fb` | `Fd = Fa - Fb` |
| `02` | `MUL Fd, Fa, Fb` | Select a 32-bit window from the signed 64-bit product `P = Fa * Fb` using the mode table below |
| `03` | `MIN Fd, Fa, Fb` | Signed minimum |
| `04` | `MAX Fd, Fa, Fb` | Signed maximum |
| `05` | `ABS Fd, Fa` | Wrapping absolute value |
| `06` | `NEG Fd, Fa` | Wrapping negation |
| `07` | `FLOOR Fd, Fa` | Round toward negative infinity by clearing the low 16 bits |
| `08` | `CEIL Fd, Fa` | Round toward positive infinity by adding raw `0x10000` to the floor if any fractional bits are set |
| `09` | `ROUND Fd, Fa` | Round to nearest integer, ties toward positive infinity. Add raw `0x8000`, wrap, then clear the low 16 bits |
| `0A` | `TRUNC Fd, Fa` | Round toward zero, retaining the Q16.16 representation |
| `0B` | `CMP Fa, Fb` | Set the pending test to the signed ordering of `Fa` and `Fb` |
| `0C` | `RCP Fd, Fa` | Approximate reciprocal. Clamp overflow to magnitude `0x7FFF_FFFF`. Input zero returns `0x7FFF_FFFF` |
| `0D` | `RSQRT Fd, Fa` | Approximate reciprocal square root. Input `Fa <= 0` returns zero |
| `0E` | `SINCOS Fd, Fa` | Approximate sine and/or cosine of angle `Fa` in radians. See the mode table below |
| `0F` | `MOV Fd, Fa` | Copy all 32 bits |
| `10..3F` | Reserved | Invalid encoding |

CMP writes no F register. Its pending test is consumed by conditional
branches and moves just as for `CMPS` and `CMPU`.

| MUL `mode[1:0]` | Mnemonic | Bits written to `Fd` |
| --- | --- | --- |
| `00` | `MUL` | `P[47:16]` (Q16.16 × Q16.16) |
| `01` | `MUL.LO32` | `P[31:0]` (integer low half) |
| `10` | `MUL.SHR8` | `P[39:8]` (Q24.8 × Q24.8) |
| `11` | `MUL.SHR24` | `P[55:24]` (Q8.24 × Q8.24) |

MUL multiplies the raw signed 32-bit operands. A result window with a
right shift of `s` has `fa + fb - s` fractional bits when the inputs have
`fa` and `fb` fractional bits. All modes discard the other product bits.
For example, `MUL.LO32 F5, F3, F4` is `D0C4 1421`.

| SINCOS `mode[1:0]` | Result |
| --- | --- |
| `00` | `Fd = sin(Fa)`, `Fd+1 = cos(Fa)`. Requires `Fd <= 62` |
| `01` | `Fd = sin(Fa)` |
| `10` | `Fd = cos(Fa)` |
| `11` | Reserved |

RCP, RSQRT, and SINCOS use lookup tables in the hidden register-file BSRAM
region and have no IEEE special values.

#### FPU AUX (opcode E)

<table>
  <thead>
    <tr>
      <th scope="col">Bit</th>
      <th scope="col">15</th><th scope="col">14</th><th scope="col">13</th><th scope="col">12</th>
      <th scope="col">11</th><th scope="col">10</th><th scope="col">9</th><th scope="col">8</th>
      <th scope="col">7</th><th scope="col">6</th><th scope="col">5</th><th scope="col">4</th>
      <th scope="col">3</th><th scope="col">2</th><th scope="col">1</th><th scope="col">0</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <th scope="row">Word 0</th>
      <td colspan="4"><code>0xE</code><br><span>[15:12]</span></td>
      <td colspan="4"><code>X</code><br><span>[11:8]</span></td>
      <td colspan="6"><code>Fa</code><br><span>[7:2]</span></td>
      <td colspan="2"><code>kind</code><br><span>[1:0]</span></td>
    </tr>
    <tr>
      <th scope="row">Word 1</th>
      <td colspan="6"><code>Fd</code><br><span>[15:10]</span></td>
      <td colspan="6"><code>subop</code><br><span>[9:4]</span></td>
      <td colspan="4"><strong>mode</strong><br><span>[3:0]</span></td>
    </tr>
  </tbody>
</table>

`X` is a four-bit GPR index and `rX = GPR[X]`. `kind = 00` selects this
operand class. The other kind values are unassigned. FLD and FST use
`mode[1:0]` to select the transfer length, with `mode[3:2] = 00`. Every integer
bridge requires `mode = 0000`.

| Subop | Mnemonic | Semantics |
| --- | --- | --- |
| `00` | `FLD Fd, [rX]` / `FLDV.N Fd, [rX]` | Load `N` consecutive F registers starting at `Fd` |
| `01` | `FST [rX], Fa` / `FSTV.N [rX], Fa` | Store `N` consecutive F registers starting at `Fa` |
| `02` | `ILO2F Fd, rX` | Copy `rX` to `Fd[15:0]`, preserving the high half |
| `03` | `IHI2F Fd, rX` | Copy `rX` to `Fd[31:16]`, preserving the low half |
| `04` | `FLO2I rX, Fa` | Copy `Fa[15:0]` to `rX` |
| `05` | `FHI2I rX, Fa` | Copy `Fa[31:16]` to `rX` |
| `06` | `I16TOF Fd, rX` | Convert signed 16-bit `rX` exactly to Q16.16 |
| `07` | `FTOI16 rX, Fa` | Truncate Q16.16 `Fa` toward zero and write the signed 16-bit integer |
| `08..3F` | Reserved | Invalid encoding |

| FLD/FST `mode[1:0]` | Lanes | Words transferred |
| --- | --- | --- |
| `00` | 1 | 2 |
| `01` | 2 | 4 |
| `10` | 3 | 6 |
| `11` | 4 | 8 |

The memory base `a` uses the `DSEG0..3` mapping selected by `GPR[X][15:14]`.
Each lane transfers the low half at word `a + 2i`, then the high half at
`a + 2i + 1`. The F-register range must fit within F0 through F63.

#### FPU geometry helpers

The v3 geometry library (`v3_length2` -> `fix32`, `v3_normalize` -> `vec3`,
`v3_distance_gt` -> `bool`, and the `_checked` debug variants) is a compiler
library, not a new opcode; its reference model lives in the CPU V3 architecture
crate, and the compiler lowers it to existing FPU v2 instructions.
`v3_length2(v)` is one `DOTSTORE` of `v` with itself. `v3_normalize(v)` is that
`DOTSTORE`, then `RSQRT`, then a scalar-vector `VMULS`; `RSQRT(0) == 0`, so a
zero vector stays zero. `v3_distance_gt(a, b, threshold)` is `VSUB`, one
`DOTSTORE` of the difference, the ordinary distance approximated as
`s * RSQRT(s)`, and the scalar `CMP` against the ordinary Q16.16 `threshold`.
The release helpers emit no checks and assume the small-range Q16.16 contract;
the `_checked` variants validate it and `halt` with a fixed nonzero signal
(1 length2 range, 2 normalize range, 3 distance input range, 4 distance
difference range) instead of clamping or returning zero. Length/normalize
require every component inclusively within `[-104, +104]` Q16.16 (so a
three-component squared sum is at most `32448`); distance first requires every
input component within `[-16384, +16383]` so the 32-bit subtraction cannot
overflow, then every difference component within `[-104, +104]` before
`DOTSTORE`. The single `DOTSTORE` narrowing and the `RSQRT`/`MUL` approximation
make the ordinary-distance boundary approximate.

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
| Reserved or malformed encoding with a defined fault (including a truncated FPU pair) | `InvalidInstruction` (code 1); the instruction does not retire |
| Conditional branch or conditional move without a pending test | `InvalidInstruction`; does not retire |
| Address outside fitted physical memory (including FLD/FST/FLDV/FSTV) | physical-address fault at the faulting offset; does not retire |
| Unaligned `LCOPY` operand | data-memory fault at the instruction offset; does not retire |
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

The compiler continues to emit 16-bit offsets. Its linked code must fit one 64K-word code window,
and static data, heap, and stack must fit one 64K-word data window. When the backend declares that
CSEG and DSEG differ, code offsets may overlap data offsets; otherwise the linker rejects overlap
for standalone unified-segment execution. `CompilerOptions::code_base` (CLI
`--target cpu-v3 --code-base`) relocates the linked code offsets without adding padding to the
output file. The offline packer places those bytes at the matching physical segment and offset.

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

## Revision 0.9

Revision 0.9 assigns major-6 function 7 to `LCOPY`, replaces the single data
segment with four 16-Kword page mappings while retaining atomic legacy `DSEG`
setup, and assigns function-E selectors 6 and 8 to `DCLEANL` and `DWAIT`;
selector 7 is reserved. These D-cache commands use the asynchronous contract
documented above.

## Revision 0.10

LCOPY keeps encoding `6 7 source destination`, but both operands are now ordinary
DSEG-mapped word pointers. It accepts different offsets/sets and writes DRAM even
for physical aliases of the same row. Earlier binaries using an eight-bit page
operand must be recompiled: an old page value128 would now mean pointer0x0080.
Opcode2/function7 (formerly DCOPYL) is reserved/invalid; its resident-destination
operation and compiler intrinsic are removed, without an alias. GPU native ABI
is unchanged.


## Revision 0.11

Unprefixed `LOAD`/`STORE` offsets are zero-extended `u4` (`0..15`) instead of
signed `i4` (`-8..7`). `PFX12` still supplies the high twelve bits of the full
16-bit displacement; negative offsets require a prefix. All other immediate
and addressing semantics are unchanged. Recompile old binaries containing
unprefixed negative memory offsets; new short offsets 8..15 require matching
revision 0.11 hardware. The core uses the existing unsigned immediate path,
with no additional instruction cycle, register or architectural state.

## Revision 0.12

Scalar MUL assigns its two low mode bits to the product window. Mode `00`
keeps the Q16.16 result, and all previously defined instruction encodings
remain valid. The new scalar MUL modes require revision 0.12 hardware.
