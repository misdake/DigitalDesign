# CPU V3 migration specification

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
- The initial Tang Nano 20K implementation fits 8 MiB of SDRAM: 23 byte-address
  bits, 22 16-bit-word-address bits, and 21 32-bit controller-beat bits. Any
  architectural physical address outside that fitted range faults instead of
  being truncated or aliased.
- `IMMHI12` and an eligible adjacent consumer form one precise two-word
  operation. A consumer fault reports the prefix address and retires neither
  word. A non-consumer expires and separately retires a pending prefix.
- `HALT` carries no result field. Host tests observe `r0`; board programs write
  their result to the system-control UART device before halting.

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

## Memory and boot direction

The first implementation keeps the CPU, cache controller, SDRAM scheduler, and
Gowin Controller HS on the same 54-MHz project clock. BSRAM contains cache data,
tags, and a small initialized boot path; SDRAM is main memory. A clock-enable
does not relax timing and is not used as a substitute for pipelining.

Program loading is a separate concern from cache operation: SDRAM has no
bitstream initialization. Immutable Stage0 code starts from initialized
BSRAM/instruction-cache state in segment zero, copies Stage1 from
SPI Flash into SDRAM, invalidates both caches entirely, writes the Stage1
data segment, and enters Stage1 with `JSEG`. Stage0 does not write a stack
pointer: each stage initializes its own from its compiled-in `--stack-init`.
Stage1 understands the extensible section table and loads the application.

Cache tags and all cache-to-SDRAM requests contain physical word addresses.

## Boot DMA device registers

Device 2 exposes channels 0..15. Stage1 programs an absolute 24-bit Flash byte address, a 22-bit physical SDRAM word
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

Device 0 is addressed only with `DEVRECV`/`DEVSEND`. Writing any value to
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
then repeatedly emits a 10-byte UART frame on channel 3: ASCII `CV3B`, stage,
category, error code, detail low byte, detail high byte, and the XOR checksum
of bytes 0 through 8. The host reference loaders expose the same mapping as
`LoaderError::boot_report` for the on-hardware stages to mirror.

The LED word has no success encoding. Independently of software, the fitted
system drives a passive boot-progress display on the same LEDs (reset held,
SDRAM initialization, Stage0, DMA, Stage1, application entry, sticky fault);
the first software write to channel 2 takes ownership permanently until
reset. All of these patterns are progress evidence only: only the
application's UART frame and system-level checks establish a successful boot
(see `systems/cpu-v3-tang-nano-20k/src/boot/FLASH_LAYOUT.md`).

## Boot-select strap device

Device 1 exposes the boot-select channels. The fitted system latches a stable
one-hot button value during reset and exposes it
after button release; channel 0 reads it. Stage1 reads the low two bits to
choose the application: button `10` selects the alternate application
section, while button `01` and the default `00` select the primary one.

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

The first arbiter permits one accepted Controller HS operation. In the host
scheduler model a due refresh has priority before accepting new client work,
data-cache traffic wins an idle tie with instruction traffic, and accepted
work runs atomically to completion. At 54 MHz the initial refresh threshold
is 600 project cycles (about 11.1 us), matching the board-characterized SDRAM
self-test with conservative margin. The fitted board system adds the boot DMA
engine as a third client of the same arbiter, with fixed priority DMA, then
data, then instruction; there refresh is handled inside the Controller HS
word-port adapter rather than by the arbiter itself.
