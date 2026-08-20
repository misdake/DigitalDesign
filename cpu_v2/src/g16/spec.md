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
BSRAM/instruction-cache state in segment zero, copies Stage1 from
SPI Flash into SDRAM, invalidates the affected physical cache lines, writes
the initial data segment and stack pointer, and enters Stage1 with `JSEG`.
Stage1 understands the extensible section table and loads the application.

Cache tags and all cache-to-SDRAM requests contain physical word addresses.

## Boot DMA device registers

Device 2 occupies offsets `0xff20..0xff2f` in the fixed MMIO page. Stage1
programs an absolute 24-bit Flash byte address, a 22-bit physical SDRAM word
destination, and file and in-memory byte sizes through literal-channel
`dev_send` calls. Writing `1` to channel 0 starts one command; channel 1
reports idle (`0`), busy (`1`), done (`2`), or error (`0x8000`). Channels 10
through 13 held the CRC32 registers before format version 3 and are free.
Channels 14 and 15 expose the error code and low completed-word count for
diagnostics. The DMA zero-fills `memory_size - file_size`, so BSS does
not require a second device command.
Error codes are stable: `1` means file size exceeds memory size, `2` means the
Flash extent is invalid, `3` means the physical-memory extent is invalid, and
`4` and `5` are Flash and SDRAM transport failures.
Consequently equal offsets in different segments never alias in a cache. DMA
writes require explicit invalidation before CPU execution or reads; changing a
segment alone does not invalidate correctly tagged lines.

## System control device and boot error reporting

Device 0 occupies offsets `0xff00..0xff0f` in the fixed MMIO page, addressed
through `r15` with the channel as a literal offset. Writing any value to
channel 0 invalidates the whole instruction cache, and to channel 1 the whole
data cache. Channel 2 drives the six board LEDs from the low six written
bits. Channel 3 accepts one UART transmit byte per write (8N1) and reports
bit 0 set on reads while the transmitter is busy.

After DMA loads and before `JSEG` the boot code must invalidate both caches
through channels 0 and 1. This is harmless at cold boot, when every valid bit
is already clear.

On failure a boot stage writes channel 2 with `{stage[1:0], category[3:0]}` in
the low six bits (stage `01` = Stage0, `10` = Stage1; category `1` =
descriptor/format invalid, `2` = manifest/section invalid, `3` = DMA
transport failure, `4` = entry/handoff invalid, `5` = internal/timeout) and
then repeatedly emits a 10-byte UART frame on channel 3: ASCII `G16B`, stage,
category, error code, detail low byte, detail high byte, and the XOR checksum
of bytes 0 through 8. The host reference loaders expose the same mapping as
`LoaderError::boot_report` for the on-hardware stages to mirror.

## First data-cache policy

The first processor uses split 2-KiB instruction and data caches. Each is
direct-mapped with 64 sets and 32 bytes (16 CPU words) per line. Each cache's
1,024 data words map exactly to one characterized 1024x16 BSRAM leaf, for two
data blocks total. Its 64 physical 12-bit tags map through a characterized
SSRAM leaf to 12 RAM16 primitives (768 physical SSRAM bits); resettable valid
bits remain ordinary registers. One arbiter shares the SDRAM transaction port;
instruction misses may not starve refresh or an already accepted data
transaction.

Reads allocate a complete line. Stores are write-through; write misses do not
allocate. This avoids dirty eviction and makes early correctness/debugging much
simpler. The first reusable RTL revision deliberately refills a line through
16 serialized physical-word transactions and converts a 16-bit store into a
one-beat 32-bit masked write. This keeps the cache, DMA, and CPU on one already
characterized word-port contract. Replacing the refill sequencer with one
Controller HS 8-beat burst is a contained throughput optimization after the
complete boot path is stable. Associativity and write-back are policy changes
behind the same CPU interface, to be justified by measured miss traffic rather
than copied from the exploratory model.

The first arbiter permits one accepted Controller HS operation. A due refresh
has priority before accepting new client work, data-cache traffic wins an idle
tie with instruction traffic, and accepted work runs atomically to completion.
At 54 MHz the initial refresh threshold is 600 project cycles (about 11.1 us),
matching the board-characterized SDRAM self-test with conservative margin.
