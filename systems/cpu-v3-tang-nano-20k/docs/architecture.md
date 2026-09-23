# CPU V3 Tang Nano 20K system architecture

This document describes the current fitted Stage 12 system. It is the current-state companion to
[`cpu-v3-optimization.md`](cpu-v3-optimization.md), which preserves the history and evidence for each
optimization Stage. Reusable processor details belong to the
[`CPU V3 IP documentation`](../../../ip/cpu-v3/docs/README.md).

## Composition boundary

`CpuV3System` is the final composition boundary for the Tang Nano 20K target. It connects:

- the revision 0.9 `CpuV3Core` at the Stage 12 microarchitecture level;
- a four-entry instruction fetch queue with a four-entry, two-word resolved-target BTC;
- a Stage0 instruction BSRAM window and separate 4-KiB I-cache and D-cache;
- the seven-owner CPU V3 memory arbiter, boot DMA, and first tile-cache GPU engine;
- the related-clock SDRAM/display port and Gowin Controller HS boundary;
- SPI-Flash boot DMA, system-control, boot-select, and framebuffer devices;
- boot-progress reporting, UART, LEDs, and the HDMI output path (compile-time
  display mode from `display::ACTIVE_DISPLAY_CONFIG`).

The system owns concrete memory layout, device indices and channels, board clocks, boot packaging,
firmware, display scheduling, and physical validation. The CPU IP sees only physical instruction and
data word ports plus the narrow device port.

## Processor and instruction path

The core is precise and in order. Stage 12 adds a conservative two-stage frontend: eligible
single-cycle sequential integer instructions accept the next queued word during Execute and can
retire at one instruction per cycle. Loads, multiply, FPU, control transfers, device operations,
invalid cases, and stores while the one-entry asynchronous store buffer is busy remain barriers.
The full rules are in [`hardware-architecture.md`](../../../ip/cpu-v3/docs/hardware-architecture.md).

The instruction fetch queue reserves at most four fetched or outstanding downstream words.
Per-slot current bits discard late responses across any number of redirects. A four-entry,
two-word resolved-target BTC can deliver a target immediately while the queue requests its
second successor. Redirects retain complete BTC entries; fault, halt, reset and global I-cache
invalidation clear them together with pending replay/fill state. Sequential fetch wraps the
16-bit PC without carrying into `CSEG`. Replacement and handshake details are specified in
[hardware-architecture.md](../../../ip/cpu-v3/docs/hardware-architecture.md).

Physical instruction words `0x00000000..0x000003ff` select the initialized Stage0 BSRAM. Other
instruction addresses use the SDRAM-backed I-cache. The I-cache is read-only and serves demand
fetches only (the Stage 6 next-line prefetch was removed after measurement); redirects and
software-controlled invalidation preserve precise handoff semantics.

## Cache and memory path

The I-cache and D-cache are independently instantiated 4-KiB, two-way caches with 64 sets and 16
16-bit words per line. Each cache uses two 1024x16 true-dual-port data BSRAMs split strictly by word
parity. Way zero and way one occupy the lower and upper halves of both parity banks. Resident reads
pipeline lookup and selected-way response for one ordered hit per cycle when there is no conflict or
backpressure. Both caches store their valid and victim bits in a RAM16 leaf with asynchronous reads:
two valid ways and the victim bit per cache, twelve 16-deep cells in total. Gowin keeps that leaf in
RAM16 only while no array write takes its way or enable from the same array's asynchronous read
data, so the victim is invalidated from the registered pending way when the line request starts
rather than from the combinationally selected victim or the request handshake. Because the RAM
cannot clear in one cycle, a global invalidate or reset clears one set of both ways
per cycle and blocks lookups for the 64-set sweep. The D-cache additionally drives a hold so the core
does not issue requests while its reset or error-scrub sweep runs.

The D-cache is write-back and write-allocate. Stores dirty resident or newly allocated lines. A dirty
victim is written back before replacement. Full clean preserves valid lines; full invalidate first
writes dirty lines and then clears validity. Dirty-line maintenance examines one 16-entry window of
the 128-bit dirty bitmap per cycle and runs that scan ahead of the in-flight write-back, so
consecutive write-backs start back to back. The system-control device holds the CPU internally until
maintenance reports success or failure. There is no per-line snoop or range-maintenance interface.

One cache line crosses the CPU-side memory interface as four ordered 64-bit beats at 54 MHz. Refill
and write-back transfer those beats directly through the parity-bank ports; neither cache retains a
private complete-line buffer. The related-clock gearbox converts a line to eight ordered 32-bit
Controller HS beats at 108 MHz. The boundary is fixed 2:1 related-clock logic, not an asynchronous
FIFO.

The fitted SDRAM is 8 MiB: 23 byte-address bits or 22 CPU word-address bits. The system rejects
larger architectural physical addresses instead of truncating or aliasing them.

`CpuV3MemoryArbiter` serializes display, boot DMA, I-cache, D-cache, GPU command reads, GPU
framebuffer reads, and GPU framebuffer writes onto the CPU-side memory port. Display has strict
priority at transaction boundaries. The other owners use base priority plus a saturating four-bit
age, with round-robin selection for equal scores; an accepted owner remains selected through its
last response or error. GPU framebuffer reads and writes are both active cache traffic. The I-cache,
D-cache, and display paths transfer fixed 4x64-bit lines; boot DMA retains its narrow-word mode.
The three GPU ports encode one through four consecutive lines as `line_count_minus_one`, giving
32/64/96/128-byte requests that must remain within one 1-KiB SDRAM row. Reads return 4/8/12/16
unstallable 64-bit beats. Long writes accept beat zero with the request and advance the source only
when the per-beat write-ready signal is asserted. `SharedSdramPort` preloads four beats and then
streams through one 8x64-bit circular 108/54-MHz gearbox while Controller HS consumes 8/16/24/32
32-bit beats; it does not duplicate the complete request in the adapter. Command and tile-list
fetches remain one line, while framebuffer cache refill and clean use four-line transactions.
An idle adapter accepts a long write even when refresh becomes due on the same cycle; the accepted
finite transaction completes first and the overdue refresh runs immediately afterward, so beat zero
cannot be lost at the 54/108-MHz gearbox boundary.
`SharedSdramPort` is a single-client line/word adapter and contains no second CPU/display arbiter.

## Clock domains

- CPU core, fetch queue, caches, arbiter, boot DMA, devices, and the CPU side of the SDRAM gearbox run
  at 54 MHz.
- Gowin Controller HS and the physical 32-bit SDRAM beat side run at the exact related 108-MHz clock.
- HDMI scanout uses separate pixel and serialization clocks. The display path owns the explicit
  crossings and line buffering; CPU IP does not depend on video clocks.

A clock enable is not treated as a timing exception or as a replacement for an explicit pipeline or
clock-domain boundary.

## Boot chain

Reset starts the single boot stage from initialized BSRAM with `CSEG = 0`,
`DSEG0..3 = {0,1,2,3}`, and `PC = 0`. The manifest's legacy `DSEG` value is a
four-page base; Stage0 writes it once to establish four consecutive mappings.
It validates the fixed package descriptor, DMAs the extensible section manifest into its own static
buffer, validates it, DMAs the reset-selected application from SPI Flash to SDRAM, initializes the
application segments and stack, and enters it through adjacent `ICACHE_INVALIDATE_ALL_DELAYED; JSEG`
instructions. There is no separate Stage1 image; the same stage understands both metadata levels.

The package format is defined in [`boot-image-format.md`](boot-image-format.md); physical Flash
placement and programming are defined in [`flash-layout.md`](flash-layout.md). The project names
exactly two RCC application sources in `boot-applications.conf`; the build derives their fixed
S1/S2 slots, entries, section layout, boot-stage selection module, pack manifest, fingerprints, and
package. Generated boot-stage, application, and package bytes come only from the system build
output. No second checked-in instruction or Flash byte array is maintained.

## Devices and ownership transfer

The fitted device allocation is:

| Device | Owner | Purpose |
| ---: | --- | --- |
| 0 | System control | I-cache invalidation, blocking D-cache maintenance, LEDs, and UART TX |
| 1 | Boot select | Latched reset-time application selection |
| 2 | Boot DMA | SPI-Flash source, SDRAM destination, length, start, and status registers |
| 3 | Display | Framebuffer configuration and scanout control |
| 4 | GPU | Two-entry submit FIFO, completion/status registers, temporary commands, and an eight-entry tile cache |

Device 0 channel 0 emits the registered one-cycle-delayed whole-I-cache invalidation pulse. Channel
1 starts blocking D-cache clean-plus-invalidate, channel 4 starts blocking D-cache clean, and channel
5 returns final maintenance status. Channel 2 writes the six logical LEDs. Channel 3 transmits one
UART byte and reports transmitter busy on reads. Channels 6 and 7 implement a generic device-value
watch: software writes `{channel[3:0], device[2:0]}` to channel 6, then writes the value it most
recently observed to channel 7. The second write holds only CPU retirement while a registered probe
continues reading the selected device; the first unequal value releases the CPU. A change between
the software read and arm is therefore detected by the first probe rather than lost. Display, DMA,
GPU, caches, and SDRAM remain clocked throughout the wait. Cache-maintenance and watch holds have
independent state so neither completion source can release the other.

Device 1 channel 0 returns the reset-time boot selection. The board-level selection latch powers up
at `10`, so the boot stage selects the configured S2 application by default; holding the S1 button
(`01`) selects the configured S1 slider diagnostic, and `11` is ignored. The current project selects
the primary diagnostic as S1 and the GPU memory-interface demo as S2. S2 alternates two permanent,
32-byte-aligned heap command buffers and the two framebuffer slots; it reports DDHT test ID `0x0b`
after each completed GPU render and display vblank. S1 reports test ID `0x07`.

Device 2 exposes the boot-DMA command and status register bank. It accepts a 24-bit absolute Flash
byte address, a 22-bit physical SDRAM word destination, and file and memory byte sizes. Writing one
to channel 0 starts a command; channel 1 reports idle (`0`), busy (`1`), done (`2`), or error
(`0x8000`). Channels 14 and 15 report the stable error code and low completed-word count. The DMA
zero-fills `memory_size - file_size`. Error codes are `1` for file size exceeding memory size, `2`
for an invalid Flash extent, `3` for an invalid physical-memory extent, and `4` or `5` for Flash or
SDRAM transport failures.

Device 4 stages a 22-bit command-buffer word address and a word count on write channels 0..3;
channel 4 submits, and channel 5 performs an idle-only soft reset or sticky-error clear. Read
channels 0..3 return accepted count, retired count, busy/full/error status, and queued depth. The
queue holds two submissions in addition to the active one. Full or malformed submissions are
rejected without incrementing the accepted count. The temporary command processor accepts only
`SET_TARGET`, `FAKE_DRAW`, and `END`; it fetches one 32-byte command or tile-list line at a time.
`FAKE_DRAW` supplies a 32-byte-aligned list of `u16` tile indices, a LOAD or CLEAR operation, two
RGB565 colors, a 16-row write mask, and a temporary tile-local XY-gradient flag. Solid and gradient
writes share the same cache path; the gradient uses channel-high bits plus local x/y and four-bit
x+y, without multipliers. The framebuffer cache is eight-entry direct-mapped and
stores eight complete 16x16 tiles in two inferred 512x32 BSRAM banks. A miss blocks while a dirty
victim is cleaned or a LOAD tile is refilled; each tile transfer is four 128-byte transactions.
`END` drains every dirty entry before incrementing the retired count, so completion fences all
visible framebuffer writes. This is a bring-up ABI, not the future geometry command-buffer
contract.

CPU, DMA, display, and GPU clients share physical SDRAM without hardware snooping. Software
transfers ownership explicitly: CPU-produced data becomes visible after blocking D-cache clean;
device-produced data becomes safely CPU-readable only after completion and blocking D-cache
invalidation. Segment changes do not provide coherence because cache tags contain physical word
addresses.

## Display and diagnostics

The application framebuffer is 400x240 RGB565 in SDRAM, arranged as 25x15 consecutive 16x16
tiles. Slots A and B begin at word addresses `0x0020_0000` and `0x0021_8000`; each reserves
`0x18000` words, including padding after the 96,000-word pixel payload. The display path issues
6,000 fixed 32-byte segment reads per source frame, stages each 4x64-bit response locally, drains
it as 8x32-bit writes into the existing dual-clock line buffers, and produces the fitted
HDMI TMDS output. The scanout mode is a single compile-time configuration
(`display::ACTIVE_DISPLAY_CONFIG`), currently 800x480@60 with a 2x upscale and no
side border; the retained 1280x720p60 3x mode is the one-word alternative and
leaves 40-pixel side borders. The framebuffer is always upscaled uniformly and
centered, so any remaining horizontal strip stays a black border. The board video
PLL follows the same switch through the example project. Boot progress owns
the six LEDs until the first software LED write, after which software owns them until reset. LED
patterns are progress evidence only; UART frames and system-level checks establish boot success or a
structured boot failure.

On boot failure, the boot stage repeatedly emits a ten-byte UART frame containing ASCII `CV3B`,
stage, category, error code, two detail bytes, and an XOR checksum. The stage byte is always `1` for
the single first stage. The LED error value combines the stage and category. The host loader exposes
the same stable mapping through `LoaderError::boot_report`.

## Current fitted result and validation boundary

The current full-system build uses 12,587 Logic (10,427 LUT, 1,536 ALU, 104 RAM16), 5,309 logic
registers, 8,603 CLS, five SDPB, four DPB, one pROM, two `MULT18X18`, one `MULT36X36`, and one
`MULTADDALU18X18`. The two additional SDPB blocks are the 512x64 framebuffer tile cache. The CPU
clock closes at 54.562 MHz against the 54-MHz constraint with 0.191 ns worst setup slack and zero
setup/hold TNS; the first setup path is an existing core-state-to-GPR-write-data path.
Controller timing closes at 122.284 MHz against 108 MHz. These are fitted implementation results,
not board evidence.

The system-level emulator-vs-RTL co-simulation `tests/system_cosim.rs` drives the composed RTL
(core, fetch queue, I-cache, D-cache, memory arbiter, and a behavioral SDRAM word port) in Icarus
against the cycle-accurate Rust system model shared with `tests/bench_emu.rs`, comparing the core
ports cycle by cycle and the post-flush SDRAM contents exactly. It is ignored by default; run it
with `cargo test -p cpu-v3-tang-nano-20k --test system_cosim -- --ignored --test-threads=1`.

This result is implementation evidence, not a substitute for board validation. Changes to clocks,
memory geometry, cache policy, SDRAM protocol, CDC, display scheduling, or resource composition must
run the corresponding hardware validation and update both this current-state document and
`cpu-v3-optimization.md`.
