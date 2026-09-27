# CPU V3 Tang Nano 20K system architecture

This document describes the current fitted Stage 12 system. It is the current-state companion to the
concise [`cpu-v3-optimization.md`](cpu-v3-optimization.md) index; long-form historical evidence lives
in [`cpu-v3-optimization-record.md`](cpu-v3-optimization-record.md). Reusable processor details belong to the
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

The [eight-DPB C16/Z16 storage and quad owner](framebuffer-cache.md) is instantiated
by the production GPU. It retains source quads through atomic masked writes and
executes in physical bank order. The controller retains blocking acquisition and
color-only SDRAM traffic; local Z initializes to far depth. Real depth/blend,
depth surface binding and concurrent sector scheduling remain separate work.

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
parity. Way zero and way one occupy the lower and upper halves of both parity banks. Each cache
also uses one 1024x16 DPB for its two 64x12 tag arrays; D-cache valid, dirty and victim
metadata occupy spare bits in that same DPB. Both tag reads launch alongside the data
reads in the existing lookup stage. Normal-mode metadata writes hold the selected port's output.
D-cache write-back assembles its address in the existing capture stage after the tag read.
The I-cache accepts one resident read per cycle without backpressure. The D-cache serializes
request acceptance, lookup and response consumption, accepting a resident request every three
cycles with a continuously ready sink. The I-cache stores valid and victim bits in twelve
explicit `RAM16SDP1` cells with asynchronous reads. The D-cache synchronously reads and
updates complete metadata words through its two DPB ports, with the victim bit stored in
way zero. The registered pending way is invalidated when its line request starts.
Because the metadata RAM cannot clear in one cycle, a global invalidate or reset clears one set of both ways
per cycle and blocks lookups for the 64-set sweep. The D-cache additionally drives a hold so the core
does not issue requests while its reset or error-scrub sweep runs.

The D-cache is write-back and write-allocate. Stores dirty resident or newly allocated lines. A dirty
victim is written back before replacement. Full clean preserves valid lines; full invalidate first
writes dirty lines and then clears validity. Maintenance scans the synchronous metadata one
set per cycle. A single pending line index overlaps this scan with write-back, preserving dense
clean throughput without a complete dirty bitmap. A conservative dirty hint skips repeated
empty full cleans; single-line clean leaves the hint set. The system-control device holds the CPU
internally until full maintenance reports success or failure. `DCLEANL` and `DWAIT` support
explicit single-line ownership handoff; there is no hardware snoop or range-maintenance interface.

One cache line crosses the CPU-side memory interface as four ordered 64-bit beats at 54 MHz. Refill
and write-back transfer those beats directly through the parity-bank ports; neither cache retains a
private complete-line buffer. The related-clock gearbox converts a line to eight ordered 32-bit
Controller HS beats at 108 MHz. The boundary is fixed 2:1 related-clock logic, not an asynchronous
FIFO.

D-cache write-back retains the locked line in its data DPBs. Request acceptance consumes only
the address; all four data beats, including beat zero, require `memory_write_data_ready`.
The DPB read address advances on that consume edge and holds during stalls, allowing consecutive
64-bit transfers without a first-beat FF copy. The arbiter forwards ready only to its accepted
owner. A held error response can terminate a partially streamed write; dirty/maintenance completion
still waits for the response. The adapter has no duplicate 256-bit fixed-line write buffer.

The fitted SDRAM is 8 MiB: 23 byte-address bits or 22 CPU word-address bits. The system rejects
larger architectural physical addresses instead of truncating or aliasing them.

`CpuV3MemoryArbiter` serializes display, boot DMA, I-cache, D-cache, GPU command reads, GPU
framebuffer reads, and GPU framebuffer writes onto the CPU-side memory port. Display has strict
priority at transaction boundaries. The other six owners use round-robin selection with one
three-bit cursor; no age or score arrays remain. An accepted owner remains selected through its
last response or error. GPU framebuffer reads and writes are both active cache traffic. The I-cache,
D-cache, and display paths transfer fixed 4x64-bit lines; boot DMA retains its narrow-word mode.
The three GPU ports encode one through four consecutive lines as `line_count_minus_one`, giving
32/64/96/128-byte requests that must remain within one 1-KiB SDRAM row. Reads return 4/8/12/16
unstallable 64-bit beats. All line writes advance the source only when the per-beat write-ready signal
is asserted. `SharedSdramPort` preloads one 64-bit holding pair, phase-aligns WRITE so Controller HS
samples its low half on the 54-MHz falling edge, and replaces it as the high half is sampled on the
following rising edge. The remaining 64-bit beats stay in their source cache entry; neither the
adapter nor gearbox contains a complete-line transaction buffer. Controller HS consumes 8/16/24/32
32-bit beats. Command and tile-list
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
| 4 | GPU | Two-entry submit FIFO, temporary commands, viewport triangle rasterizer, and an eight-entry tile cache |

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
the primary diagnostic as S1 and the GPU raster demo as S2. S2 alternates two permanent,
32-byte-aligned heap command buffers and the two framebuffer slots; it clears each slot to black
once, then renders only the viewport triangle and reports DDHT test ID `0x0b`
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
rejected without incrementing the accepted count. The temporary command processor accepts
`SET_TARGET`, `FAKE_DRAW`, `TRIANGLE`, and `END`; it fetches one 32-byte command or tile-list line at a time.
`FAKE_DRAW` supplies a 32-byte-aligned list of `u16` tile indices, a LOAD or CLEAR operation, two
RGB565 colors, a 16-row write mask, and a temporary tile-local XY-gradient flag. Solid and gradient
writes share the same cache path; the gradient uses channel-high bits plus local x/y and four-bit
x+y, without multipliers. The framebuffer cache is eight-entry direct-mapped and
stores eight complete 16x16 C16/Z16 tiles in eight explicit true-dual-port DPBs.
Bank selection is `(x + 2*y) mod 4`; each plane transfers horizontal 64-bit memory
beats and the render owner reads/writes a masked quad in two clocks.
A miss blocks while a dirty
victim is cleaned or a LOAD tile is refilled; each tile transfer is four 128-byte transactions.
`TRIANGLE` (`0xe2`) is four qwords: a zero-argument header followed by three viewport vertices,
each packed as `{y:s12.4, x:s12.4}` in the low 32 bits of one qword. The rasterizer emits covered
RGB565 quads in tile order. A ready/valid prefetch acquisition admits one outstanding tile (K=1);
the source FIFO head is retained until the owner completes the write. Uncovered lanes
retain the LOAD contents. The Phase-1 pixel record carries a draw epoch and triangle ID. The draw
marker follows all accepted pixels and remains at the raster boundary until the cache returns the
matching epoch/triangle retirement ACK; the command processor then waits for scene done.
`END` drains every dirty entry before incrementing the retired count, so completion fences all
visible framebuffer writes. This is a bring-up ABI, not the future geometry command-buffer
contract.

The default coverage producer tests two pixels per clock, with one-pixel and experimental
scanline alternatives measured in [gpu-raster-comparison.md](gpu-raster-comparison.md).
Its quad/marker FIFO explicitly uses BSRAM. A locked viewport setup record is shared across
compact tile jobs; the full-geometry path retains its separate setup/fan records. Command and
tile-list lines occupy independent halves of one RAM16 array, sharing the serial FSM's port;
cache tags also share their lookup/clean read port. Cache memory-port reads are enabled during
clean, preserving the four independent render banks. This does not add concurrent refill/render
or the earlier prefetch intended for a future pixel-processing pipeline.

CPU, DMA, display, and GPU clients share physical SDRAM without hardware snooping. Software
transfers ownership explicitly: CPU-produced data becomes visible after blocking D-cache clean;
device-produced data becomes safely CPU-readable only after completion and blocking D-cache
invalidation. Segment changes do not provide coherence because cache tags contain physical word
addresses.

## Display and diagnostics

The application framebuffer is 400x240 RGB565 in SDRAM, arranged as 25x15 consecutive 16x16
tiles. Slots A and B begin at word addresses `0x0020_0000` and `0x0021_8000`; each reserves
`0x18000` words, including padding after the 96,000-word pixel payload. The display path issues
6,000 fixed 32-byte segment reads per source frame, fetching each tile's adjacent two source rows
consecutively as two separate transactions. Every 64-bit response beat writes two dual-clock
512x32 BSRAM banks directly, with no response capture buffer or eight-cycle drain. Four rows
occupy qword addresses 0..399 in two groups of two. Publication toggles cross through two-stage
synchronizers; group metadata is stable until release. The consumer releases a group only after
the second row's final vertical repeat. An unavailable row is black and sets sticky underflow;
an error or malformed LAST stops filling without publishing the incomplete group. Reset clears
the faults and restores producer/consumer ownership.

Each bank's spare addresses 448..511 contain a 64-entry linear-to-sRGB table, one byte per
32-bit word. Green indexes its six bits; red/blue replicate five bits to six. Values evaluate
the sRGB transfer at `i/63`, rounded to nearest 8-bit output. Device 3 channel 4 writes the staged
format bit (`0`: encoded RGB565, the reset default; `1`: linear RGB565) and reads the active bit.
NEXT_SWAP snapshots format and address together; later staging cannot change a pending swap.
The host display-device model and renderer follow the same selection.

The scanout mode is a single compile-time configuration
(`display::ACTIVE_DISPLAY_CONFIG`), selected by mutually exclusive Cargo features
`display-2x` (the default when neither is specified) or `display-3x`. The default
is 800x480@60 with a 2x upscale and no
side border; the retained 1280x720p60 3x mode is the other build option and
leaves 40-pixel side borders. The framebuffer is always upscaled uniformly and
centered, with encoded dark-gray borders (`BORDER_COLOR`). The board video
PLL follows the same switch through the example project. Boot progress owns
the six LEDs until the first software LED write, after which software owns them until reset. LED
patterns are progress evidence only; UART frames and system-level checks establish boot success or a
structured boot failure.

The pixel-clock conversion pipeline starts a pair every four clocks when the completed-pair
queue has room. Each row starts reading sixteen clocks before its first framebuffer output.
A separate four-entry, 48-bit asynchronous-read FIFO stores completed RGB888 pairs in
twelve RAM16 cells. The occupancy guard reserves space for the in-flight conversion.
Scanout consumes a pair every four clocks in 2x or six clocks in 3x, and one 48-bit output
register holds the popped pair for the remaining repeats. SDRAM latency is absorbed by
the published line groups before conversion begins.

Display retains strict priority. A caught-up refill is 50 segments per two source rows;
initial fill is at most 100 segments. The conditional ideal scheduling estimate and its
refresh/contention limitations are in the upgrade comparison (archived work-1 report).

On boot failure, the boot stage repeatedly emits a ten-byte UART frame containing ASCII `CV3B`,
stage, category, error code, two detail bytes, and an XOR checksum. The stage byte is always `1` for
the single first stage. The LED error value combines the stage and category. The host loader exposes
the same stable mapping through `LoaderError::boot_report`.

## Current fitted result and validation boundary

The default 2x four-line display/sRGB system retains the eight-DPB framebuffer and
quad owner. The latest production fit adds CPU/cache architecture consolidation to
`eb41760`; its generated GPU, display and SDRAM backend sources are unchanged.

| Production fit | Logic | LUT | ALU | RAM16 | Logic FF | BSRAM | CPU fitted fmax | Worst setup slack |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Display work-1, four color cache DPBs | 13,986 | 11,182 | 2,144 | 110 | 5,618 | 14 | 57.469 MHz | 1.118 ns |
| Display + eight-DPB C/Z quad owner | 14,152 | 11,317 | 2,175 | 110 | 5,620 | 18 | 54.260 MHz | 0.089 ns |
| Same GPU + fetch/arbiter/cache-tag optimization | 13,478 | 10,963 | 2,143 | 62 | 5,493 | 20 | 56.218 MHz | 0.730 ns |
| Same system + source-held D-cache writeback | 13,380 | 10,866 | 2,142 | 62 | 5,174 | 20 | 58.403 MHz | 1.396 ns |
| **CPU/cache architecture consolidation** | **12,093** | **9,833** | **1,960** | **50** | **4,896** | **20** | **58.800 MHz** | **1.512 ns** |

The consolidation shares scalar/vector FPU lane control and ALU commitment, integer
operands/arithmetic, ordered fetch position and D-cache metadata, and replaces age
scores with a six-client round-robin cursor. It removes 1,287 Logic without adding RAM
or DSP. The current fit uses 7,899 CLS, five SDPB, fourteen DPB, one pROM, two `MULT18X18`,
one `MULT36X36` and five `MULTADDALU18X18`: sixteen 18x18-equivalent multiplier lanes,
reported as 34% DSP utilization. GPU storage is eight cache DPBs plus one raster
FIFO SDPB; command/list and tags share 18 RAM16 cells. Real shader/depth/blend and depth
surface traffic are outside this fit; the reserved Z DPBs are included.

Runtime clocks remain 54/108 MHz, with zero setup/hold TNS and violated endpoints.
The first setup path is core instruction decode to fetch metadata-current clock enable;
controller timing closes at 145.872 MHz against 108 MHz. Place/route algorithms remain 1.
All twenty hardware-validation steps, 732 workspace tests, strict Clippy, 31 CPU ignored
RTL tests, two system co-sims, 22 system RTL tests and both full Flash-image tests passed.
All 22 frozen benchmarks retain their execution cycles; including one final full clean
per program increases geometric-mean completion cycles by 0.34%; the worst case is the
short FPU spill stress at +4.94% (61 clocks). Sparse clean pays a
bounded set scan; dense full clean retains write-back throughput. Sources, matched fits,
independent goldens and raw performance are archived in local record
`cpu-v3-architecture-logic-2026-09-28`. This result is not new physical-board proof.

## Verification and physical evidence

The system-level emulator-vs-RTL co-simulation `tests/system_cosim.rs` drives the composed RTL
(core, fetch queue, I-cache, D-cache, memory arbiter, and a behavioral SDRAM word port) in Icarus
against the cycle-accurate Rust system model shared with `tests/bench_emu.rs`, comparing the core
ports cycle by cycle and the post-flush SDRAM contents exactly. It is ignored by default; run it
with `cargo test -p cpu-v3-tang-nano-20k --test system_cosim -- --ignored --test-threads=1`.
Differential equality is not treated as a semantic oracle by itself: cache-command scenarios also
check explicit destination values, and GPU memory-effect tests start from nonzero sentinels, require
known nonzero output pixels, and retain an unchanged guard word outside the framebuffer payload.
GPU raster tests compare a four-tile crop and 26 complete 400x240 RGB565 scenes against an
independent integer pixel-center/top-left oracle. They check exact pixel/ACK conservation, stalled
ports, alias eviction, LOAD/CLEAR preservation, guards, and terminal errors at all 36 read/write
beat positions. The reproducible scene and throughput suite is `tests/gpu_trace_cosim.rs`.
The earlier tile-display path and cold boot are user-confirmed. The triangle-only application passes
full-frame Flash RTL checks for both slots, including nonzero initial sentinels and payload guards.
For this display upgrade, 727 workspace tests, strict Clippy, five display RTL tests, both
video-mode PnR/audits, 26 CPU and two system co-sims pass. Both complete Flash RTL tests and
four top-level resource tests also pass; this upgrade has no new physical-board evidence.
The previous `11729ac` image passed cold-boot UART/HDMI validation. The Logic closure
image at `6149698` passed audited SRAM loading, then complete Flash Program/Verify
at `0x000000` (boot package at `0x100000`) and another SRAM load. Each UART capture
passes 499 strict S2 `0x0b` success frames with zero errors; BL616 recovery was unnecessary.
That earlier optimized image remains in Flash and SRAM; its cold-boot UART/HDMI check is pending.
K=1 and blocking refill/clean remain; geometry and varying interpolation are not implemented.

This result is implementation evidence, not a substitute for board validation. Changes to clocks,
memory geometry, cache policy, SDRAM protocol, CDC, display scheduling, or resource composition must
run the corresponding hardware validation and update both this current-state document and
`cpu-v3-optimization.md`.
