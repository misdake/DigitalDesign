# CPU V3 Tang Nano 20K system architecture

This document describes the current fitted Stage 12 system. It is the current-state companion to
[`cpu-v3-optimization.md`](cpu-v3-optimization.md), which preserves the history and evidence for each
optimization Stage. Reusable processor details belong to the
[`CPU V3 IP documentation`](../../../ip/cpu-v3/docs/README.md).

## Composition boundary

`CpuV3System` is the final composition boundary for the Tang Nano 20K target. It connects:

- the revision 0.7 `CpuV3Core` at the Stage 12 microarchitecture level;
- a four-entry epoch-tagged instruction fetch queue;
- a Stage0 instruction BSRAM window and separate 4-KiB I-cache and D-cache;
- the CPU V3 memory arbiter and boot DMA client;
- the related-clock SDRAM/display port and Gowin Controller HS boundary;
- SPI-Flash boot DMA, system-control, boot-select, and framebuffer devices;
- boot-progress reporting, UART, LEDs, and the 720p HDMI output path.

The system owns concrete memory layout, device indices and channels, board clocks, boot packaging,
firmware, display scheduling, and physical validation. The CPU IP sees only physical instruction and
data word ports plus the narrow device port.

## Processor and instruction path

The core is precise and in order. Stage 12 adds a conservative two-stage frontend: eligible
single-cycle sequential integer instructions accept the next queued word during Execute and can
retire at one instruction per cycle. Loads, multiply, FPU, control transfers, device operations,
invalid cases, and stores while the one-entry asynchronous store buffer is busy remain barriers.
The full rules are in [`hardware-architecture.md`](../../../ip/cpu-v3/docs/hardware-architecture.md).

The instruction fetch queue reserves at most four fetched or outstanding words and tags downstream
requests with an epoch. Redirect, fault, halt, reset, or global I-cache invalidation flushes visible
old-epoch work and drains late responses without making them architectural. Sequential fetch wraps
the 16-bit PC without carrying into `CSEG`.

Physical instruction words `0x00000000..0x000003ff` select the initialized Stage0 BSRAM. Other
instruction addresses use the SDRAM-backed I-cache. Stage0 traffic is excluded from next-line
prefetch. The I-cache is read-only; redirects and software-controlled invalidation preserve precise
handoff semantics.

## Cache and memory path

The I-cache and D-cache are independently instantiated 4-KiB, two-way caches with 64 sets and 16
16-bit words per line. Each cache uses two 1024x16 true-dual-port data BSRAMs split strictly by word
parity. Way zero and way one occupy the lower and upper halves of both parity banks. Resident reads
pipeline lookup and selected-way response for one ordered hit per cycle when there is no conflict or
backpressure.

The D-cache is write-back and write-allocate. Stores dirty resident or newly allocated lines. A dirty
victim is written back before replacement. Full clean preserves valid lines; full invalidate first
writes dirty lines and then clears validity. The system-control device holds the CPU internally until
maintenance reports success or failure. There is no per-line snoop or range-maintenance interface.

One cache line crosses the CPU-side memory interface as four ordered 64-bit beats at 54 MHz. Refill
and write-back transfer those beats directly through the parity-bank ports; neither cache retains a
private complete-line buffer. The related-clock gearbox converts a line to eight ordered 32-bit
Controller HS beats at 108 MHz. The boundary is fixed 2:1 related-clock logic, not an asynchronous
FIFO.

The fitted SDRAM is 8 MiB: 23 byte-address bits or 22 CPU word-address bits. The system rejects
larger architectural physical addresses instead of truncating or aliasing them.

`CpuV3MemoryArbiter` serializes I-cache, D-cache, and boot-DMA transactions onto the CPU-side memory
port. Accepted work runs to completion. `DisplaySdramPort` then schedules that traffic with display
scanout and exposes the fitted 64-bit line or narrow-word interface toward the 108-MHz SDRAM
controller. Display urgency protects scanout deadlines without changing CPU cache semantics.

## Clock domains

- CPU core, fetch queue, caches, arbiter, boot DMA, devices, and the CPU side of the SDRAM gearbox run
  at 54 MHz.
- Gowin Controller HS and the physical 32-bit SDRAM beat side run at the exact related 108-MHz clock.
- HDMI scanout uses separate pixel and serialization clocks. The display path owns the explicit
  crossings and line buffering; CPU IP does not depend on video clocks.

A clock enable is not treated as a timing exception or as a replacement for an explicit pipeline or
clock-domain boundary.

## Boot chain

Reset starts Stage0 from initialized BSRAM with `CSEG = 0`, `DSEG = 0`, and `PC = 0`. Stage0 validates
the fixed package descriptor, uses the boot DMA to copy Stage1 from SPI Flash to SDRAM, performs the
required cache-maintenance handoff, initializes Stage1 state, and enters it through adjacent
`ICACHE_INVALIDATE_ALL_DELAYED; JSEG` instructions. Stage1 parses the extensible section manifest,
loads the selected application, initializes its segments and stack, and performs the same terminal
handoff.

The package format is defined in [`boot-image-format.md`](boot-image-format.md); physical Flash
placement and programming are defined in [`flash-layout.md`](flash-layout.md). Generated Stage0,
Stage1, application, and package bytes come only from the system build output. No second checked-in
instruction or Flash byte array is maintained.

## Devices and ownership transfer

The fitted device allocation is:

| Device | Owner | Purpose |
| ---: | --- | --- |
| 0 | System control | I-cache invalidation, blocking D-cache maintenance, LEDs, and UART TX |
| 1 | Boot select | Latched reset-time application selection |
| 2 | Boot DMA | SPI-Flash source, SDRAM destination, length, start, and status registers |
| 3 | Display | Framebuffer configuration and scanout control |

Device 0 channel 0 emits the registered one-cycle-delayed whole-I-cache invalidation pulse. Channel
1 starts blocking D-cache clean-plus-invalidate, channel 4 starts blocking D-cache clean, and channel
5 returns final maintenance status. Channel 2 writes the six logical LEDs. Channel 3 transmits one
UART byte and reports transmitter busy on reads.

Device 1 channel 0 returns the reset-time boot selection. Stage1 selects the alternate application
for button value `10`; `00` and `01` select the primary application.

Device 2 exposes the boot-DMA command and status register bank. It accepts a 24-bit absolute Flash
byte address, a 22-bit physical SDRAM word destination, and file and memory byte sizes. Writing one
to channel 0 starts a command; channel 1 reports idle (`0`), busy (`1`), done (`2`), or error
(`0x8000`). Channels 14 and 15 report the stable error code and low completed-word count. The DMA
zero-fills `memory_size - file_size`. Error codes are `1` for file size exceeding memory size, `2`
for an invalid Flash extent, `3` for an invalid physical-memory extent, and `4` or `5` for Flash or
SDRAM transport failures.

CPU, DMA, display, and future GPU clients share physical SDRAM without hardware snooping. Software
transfers ownership explicitly: CPU-produced data becomes visible after blocking D-cache clean;
device-produced data becomes safely CPU-readable only after completion and blocking D-cache
invalidation. Segment changes do not provide coherence because cache tags contain physical word
addresses.

## Display and diagnostics

The application framebuffer is 320x240 RGB565 in SDRAM. The display path fetches it through the
shared SDRAM port, buffers scanout lines, and produces the fitted 720p TMDS output. Boot progress owns
the six LEDs until the first software LED write, after which software owns them until reset. LED
patterns are progress evidence only; UART frames and system-level checks establish boot success or a
structured boot failure.

On boot failure, Stage0 or Stage1 repeatedly emits a ten-byte UART frame containing ASCII `CV3B`,
stage, category, error code, two detail bytes, and an XOR checksum. The LED error value combines the
two-bit stage and four-bit category. The host loader exposes the same stable mapping through
`LoaderError::boot_report`.

## Current fitted result and validation boundary

The Stage 12 full-system build uses 10,100 Logic (8,822 LUT, 750 ALU, 88 SSRAM), 4,324 registers,
four DPB, one SDPB, two pROM, and two `MULT18X18` cells. The CPU clock closes at 56.230 MHz against
the 54-MHz constraint with zero setup and hold TNS. The tightest CPU path is the fetch-queue to
I-cache way-valid route.

This result is implementation evidence, not a substitute for board validation. Changes to clocks,
memory geometry, cache policy, SDRAM protocol, CDC, display scheduling, or resource composition must
run the corresponding hardware validation and update both this current-state document and
`cpu-v3-optimization.md`.
