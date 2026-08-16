# G16 migration specification

This directory is the executable source of truth for the ISA replacing cpu_v2
v2.6. The exploratory `design_model/CPU_ISA.md` revision 0.2 supplied the base
encoding. This repository owns later revisions so builds never depend on a
machine-local project.

The self-contained visual reference is [`isa.html`](isa.html). It documents
the complete encoding alongside the compiler ABI, unified memory map, cache
organization, and the explicitly separated current FPGA implementation status.

## Architectural boundary

- Instructions and data use 16-bit word offsets within boot-selected code and
  data segments. A physical word address is the direct concatenation
  `{segment[15:0], offset[15:0]}`; segment arithmetic is never added to the
  offset and offset wrap never advances a segment.
- There are sixteen writable 16-bit GPRs and no architectural flags.
- `r0..r1` return values, `r2..r7` arguments, `r8..r11` callee-saved values,
  `r12` compiler scratch, `r13` stack pointer, `r14` link, and `r15` the
  global/MMIO base.
- `CSEG` supplies the high physical bits for instruction fetch. `DSEG` supplies
  them for ordinary loads and stores, including stack accesses. The fixed MMIO
  offset page `0xff00..0xffff` always selects system space in segment zero.
- Reset establishes `CSEG = 0`, `DSEG = 0`, and `PC = 0`. Normal applications
  do not change either segment. Stage0 writes `DSEG` immediately before an
  atomic segmented jump establishes the application `CSEG` and entry offset.
- The initial Tang Nano 20K implementation fits 8 MiB of SDRAM: 23 byte-address
  bits, 22 16-bit-word-address bits, and 21 32-bit controller-beat bits. Any
  architectural physical address outside that fitted range faults instead of
  being truncated or aliased.
- `IMMHI12` and an eligible adjacent consumer form one precise two-word
  operation. A consumer fault reports the prefix address and retires neither
  word. A non-consumer expires and separately retires a pending prefix.
- `HALT` carries no result field. Host tests observe `r0`; board programs write
  their result to UART/MMIO before halting.

The baseline offset map inside the selected data segment is:

| Range | Initial use |
| --- | --- |
| `0x0000..` | linked code, growing upward |
| `0x4000..` | static data |
| `0x8000..` | heap baseline |
| below `0xff00` | stack, growing downward |
| `0xff00..0xffff` | fixed MMIO page in segment zero, addressed through `r15` |

`CompilerOptions::g16()` selects these boundaries. The old v2.6 convention of
using a zero stack pointer to wrap to `0xffff` is rejected because that address
is MMIO under G16.

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

Revision 0.4 makes the complete fitted SDRAM addressable without widening
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
data window. `CompilerOptions::code_base` (CLI `--g16 --code-base`) relocates
the linked code offsets without adding padding to the output file. The offline
packer places those bytes at the matching physical segment and offset.

## Memory and boot direction

The first implementation keeps the CPU, cache controller, SDRAM scheduler, and
Gowin Controller HS on the same 54-MHz project clock. BSRAM contains cache data,
tags, and a small initialized boot path; SDRAM is main memory. A clock-enable
does not relax timing and is not used as a substitute for pipelining.

Program loading is a separate concern from cache operation: SDRAM has no
bitstream initialization. Immutable Stage0 code starts from initialized
BSRAM/instruction-cache state in segment zero, copies and verifies Stage1 from
SPI Flash into SDRAM, invalidates the affected physical cache lines, writes
the initial data segment and stack pointer, and enters Stage1 with `JSEG`.
Stage1 understands the extensible section table and loads the application.

Cache tags and all cache-to-SDRAM requests contain physical word addresses.

## Boot DMA device registers

Device 2 occupies offsets `0xff20..0xff2f` in the fixed MMIO page. Stage1
programs an absolute 24-bit Flash byte address, a 22-bit physical SDRAM word
destination, file and in-memory byte sizes, and the expected CRC32 through
literal-channel `dev_send` calls. Writing `1` to channel 0 starts one command;
channel 1 reports idle (`0`), busy (`1`), done (`2`), or error (`0x8000`).
Channels 12 through 15 expose the actual CRC, error code, and low completed-word
count for diagnostics. The DMA zero-fills `memory_size - file_size`, so BSS does
not require a second device command.
Consequently equal offsets in different segments never alias in a cache. DMA
writes require explicit invalidation before CPU execution or reads; changing a
segment alone does not invalidate correctly tagged lines.

## First data-cache policy

The first processor uses split 2-KiB instruction and data caches. Each is
direct-mapped with 64 sets and 32 bytes (16 CPU words) per line. Each cache's
1,024 data words map exactly to one characterized 1024x16 BSRAM leaf, for two
data blocks total. Tags and valid bits are small enough for logic registers
initially. One arbiter shares the SDRAM transaction port; instruction misses
may not starve refresh or an already accepted data transaction.

Reads allocate a complete line. Stores are write-through; write misses do not
allocate. This avoids dirty eviction and makes early correctness/debugging much
simpler. The target SDRAM adapter converts a line refill into one Controller HS
8-beat 32-bit burst, and converts a 16-bit store into a one-beat 32-bit masked
write. Associativity and write-back are policy changes behind the same CPU and
line-transaction interfaces, to be justified by measured miss traffic rather
than copied from the exploratory model.

The first arbiter permits one accepted Controller HS operation. A due refresh
has priority before accepting new client work, data-cache traffic wins an idle
tie with instruction traffic, and accepted work runs atomically to completion.
At 54 MHz the initial refresh threshold is 600 project cycles (about 11.1 us),
matching the board-characterized SDRAM self-test with conservative margin.
