# G16 migration specification

This directory is the executable source of truth for the ISA replacing cpu_v2
v2.6. The exploratory `design_model/CPU_ISA.md` revision 0.2 supplied the base
encoding. This repository owns later revisions so builds never depend on a
machine-local project.

The self-contained visual reference is [`isa.html`](isa.html). It documents
the complete encoding alongside the compiler ABI, unified memory map, cache
organization, and the explicitly separated current FPGA implementation status.

## Architectural boundary

- Instructions and data share one 16-bit word-addressed, 65,536-word space.
- There are sixteen writable 16-bit GPRs and no architectural flags.
- `r0..r1` return values, `r2..r7` arguments, `r8..r11` callee-saved values,
  `r12` compiler scratch, `r13` stack pointer, `r14` link, and `r15` the
  global/MMIO base.
- The initial FPGA implementation exposes a 128-KiB CPU-visible SDRAM window.
  Mapping the complete fitted SDRAM is deferred until software needs it.
- `IMMHI12` and an eligible adjacent consumer form one precise two-word
  operation. A consumer fault reports the prefix address and retires neither
  word. A non-consumer expires and separately retires a pending prefix.
- `HALT` carries no result field. Host tests observe `r0`; board programs write
  their result to UART/MMIO before halting.

The baseline unified-memory map is:

| Range | Initial use |
| --- | --- |
| `0x0000..` | linked code, growing upward |
| `0x4000..` | static data |
| `0x8000..` | heap baseline |
| below `0xff00` | stack, growing downward |
| `0xff00..0xffff` | MMIO page addressed through `r15` |

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

## Memory and boot direction

The first implementation keeps the CPU, cache controller, SDRAM scheduler, and
Gowin Controller HS on the same 54-MHz project clock. BSRAM contains cache data,
tags, and a small initialized boot path; SDRAM is main memory. A clock-enable
does not relax timing and is not used as a substitute for pipelining.

Program loading is a separate concern from cache operation: SDRAM has no
bitstream initialization. The first board milestone may copy a linked image
from initialized BSRAM before releasing the CPU. A UART loader can replace that
bootstrap later without changing the ISA or cache protocol.

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
