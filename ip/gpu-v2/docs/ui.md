# Fixed-grid UI drawing

UI_GRID draws a400x240 RGB565 color plane as50x15 cells of8x16 pixels.
Each cell names one char (text or icon); IDs0..254 are valid and FF skips
the cell. Alpha is binary: zero preserves the destination and one replaces it.
There is no depth access, filtering, scaling, lighting or fractional blending.

## Assets and command

The driver provides a750-byte row-major ID array, with readable padding to768
bytes and a128-byte-aligned base. The atlas is64-KiB aligned and allocated as
256 entries of256 bytes; entry255 is never read. Both assets must remain stable
and disjoint from the target throughout execution, within the8-MiB GPU range.

Each char contains128 little-endian RGBA5551 words: R15:11, G10:6, B5:1, A0.
Rows are16 bytes. Within each group of four pixels, odd rows store logical
pixels2,3,0,1 instead of0,1,2,3. This offline ordering directly matches the four
framebuffer banks. `ui_char` converts ordinary row-major pixels; hardware needs
no pixel permutation. RGB565 green is `{G5,G5[4]}`; red and blue retain five bits.

UI_GRID is opcode11 with the same staging-slot operand as SET_TARGET. It reads
one qword from a32-byte object: grid byte base/128 in bits15:0, atlas byte
base/65536 in bits22:16, and zero reserved bits. The remaining object bytes are
zero padding. `ui_params` checks extents and alignment; `Command::ui_grid`
encodes the command. The canvas is the color base installed by SET_TARGET.

`Command::ui_grid_range(qword, first, end)` selects a nonempty row-major interval
`[first, end)` within0..750. The same32-bit instruction carries bit27=1,
first in16:7, inclusive last (`end-1`) in26:17, and the descriptor slot in6:0.
Bit27=0 retains the original full-grid encoding with26:7 zero. These fields
remain in the existing CP instruction register until the UI call completes;
there is no separately installed UI range or saved cursor. A rectangle can be
expressed as one interval per character row. Empty intervals are omitted by
the producer.

The Rust helper takes the descriptor's local64-bit qword index, like
`Command::ui_grid`: qword276 encodes slot69 in command bits6:0. The descriptor
layout is unchanged. For example, `Command::ui_grid_range(276, 1, 4)` draws
cells1,2,3 using the descriptor at local qword276.

The grid allocation and alignment remain unchanged. Only the selected bytes
need meaningful IDs for a range call: DMA reads the intersecting128-byte grid
sectors into their original scratchpad positions, and the checker starts at the
containing eight-ID window. At most three initial pairs are skipped using its
existing fixed16-bit progression. Edge masks force out-of-range IDs to FF,
including either half of a shared tile; they cause no atlas read or pixel write.
No variable shift, packed-grid relocation or per-row address conversion is used.

The driver explicitly WAITs for all rendering and data DMA, then FLUSHes the
framebuffer cache before UI_GRID. The command is serial and completes only
after all its framebuffer write terminals. It does not change global/render
bindings or depth. Both meshlet input pages become invalid and must be reloaded
before later DRAW commands. Uniform, metadata and command-page storage survive.
No automatic present, clear or texture invalidation is performed.

Consecutive UI calls need no intervening WAIT, FLUSH or UI-state maintenance:
each call reloads its own selected grid sectors and atlas entries, starts a new
checker, and retires its final physical writes before the next call executes.
The initial transition from3D still needs the explicit WAIT/FLUSH above. A
masked-out neighbor in a shared16x16 tile is preserved by the background read.

The current RCC Pass APIs retain the full-grid command (`0xb0000045` for
qword276). Both snapshot recording and in-place begin/write/end construction
remain compatible. In-place `gpu_pass_ui_write` selects which cells to edit in
the private object; its first/count arguments do not encode a hardware draw
range. That driver still emits WAIT/FLUSH for each UI object. The optional
hardware range and omitted fences between consecutive UI commands are exposed
by the Rust command helper and Python command producer, independently of that
conservative RCC recording policy. See the [driver](/systems/cpu-v3-tang-nano-20k/docs/gpu-driver.md#in-place-ui-construction).

## Storage and pipeline

The input scratchpad is reused without extra BSRAM: page0 holds the768-byte
grid, while page1 holds two512-byte char-pair buffers. Two ordered task slots
hold `{tile_index9,left_ID8,right_ID8}` and validity/readiness; each slot retains
its associated buffer until the task's final write terminal. Initialization's
grid-base field shares the low16 bits of the later scanner window because
their lifetimes do not overlap.

The checker reads eight IDs per synchronous64-bit read, then examines one pair
per enabled edge using fixed16-bit progression. FF/FF pairs produce no task or
framebuffer traffic. The two tasks cover the current and next tile. Atlas reads
use the existing command DMA: two128-byte bursts per non-FF char, directly into
the selected task's final buffer. If the next task is already queued or is
being pushed by the checker, its atlas loading finishes before the current
background read starts. This groups atlas and framebuffer reads into separate
same-master runs rather than alternating them through the shared arbiter.
Without a known next task, the current task proceeds immediately; the final
task never waits for a nonexistent successor. Slots release only at final write
retirement, so the following atlas group cannot overwrite either live buffer.
Command prefetch pauses during UI; the checker and char consumer share the
source read port in fixed phases, with no port-request arbiter.

```mermaid
flowchart TD
    Grid["Scratchpad page0: grid"] --> Checker["Pair checker / two ordered tasks"]
    Checker --> DMA["Existing command DMA / next atlas pair"]
    DMA --> Chars["Page1: two task-owned char buffers"]
    Chars --> A["Four pixels per edge / independent alpha write enables"]
    BG["Existing framebuffer refill / color only"] --> Banks["Existing framebuffer banks / two fixed half-tiles"]
    A --> Banks
    Banks --> WB["Existing B-port writeback / held native Q"]
    WB --> Mem["Framebuffer-write master / shared MC"]
    Mem -->|"Actual final terminal releases task"| Checker
```

Each background read is one four-sector maintenance operation for the entire
16x16 color tile. All four requests advance through the existing issue cursor
without an upper/lower command handoff. The second actual read terminal starts
A's upper overlay, four pixels per enabled edge, while B fills the disjoint
lower half. The fourth terminal releases read maintenance. Once the upper
overlay's final pixel Q is consumed, a single four-sector writeback begins,
overlapping A's lower overlay with B's upper writeback.

The lower half's write requests and B reads are gated until its overlay is
complete. This readiness is stable after publication and ignores checker Qs;
only a tagged pixel Q can extend a pixel's lifetime. An arbitrary CE pause can
therefore delay the lower-half requests without stalling an already accepted
write burst. B's valid bit clears if its old beat is consumed without a next
read, and reloads when production resumes. Abort opens the lower gate so all
accepted work drains using existing background data where necessary.

UI uses the ordinary four-sector issue/data/retire counters, but maps both
sector-index halves to the installed color base and contiguous UI bank words.
Depth addressing is disabled for UI. There is no extra RAM, payload queue,
upper/lower command pair or second output-sector buffer. Native B Q holds
through write-data stalls and advances only after acceptance.
UI temporary lines are never published as normal cache residents. Depth is not
refilled or written. Accepted memory work drains independently of compute CE;
completion remains visible until the decoder consumes it. Explicit abort drains
accepted DMA/maintenance work without reporting successful CB completion.
Each alpha bit gates an entire independent 16-bit bank write. Transparency does
not use a partial native BSRAM write or require a read-modify-write of a pixel.

## Focused verification

`tests/ui_grid.py` compares the entire color plane to independent screen-space
glyph equations and verifies unchanged depth using the real native GPU, shared
MC and SDRAM pin model. It covers sparse/dense/empty grids, ID0/254, one-sided
FF, binary transparency, edge cells, CE pauses, repeated UI commands and abort.
Mode3 holds CE low for80 edges after the first write request, exercising the
lower-half publication and data gate. `--ranges first:end ...` runs consecutive
independent range calls, comparing
the entire color plane including excluded neighbors and checking the exact
number of grid/atlas read requests and framebuffer requests. Ranges can cross
character-table sectors; empty-content ranges emit no framebuffer work.
The bounded runner records exact DUT/peer hashes. Its standalone MC profile and
cycles are separate from full-system CL3 timing and physical-board qualification.

`tests/ui_draw_transition.py` executes DRAW, WAIT/FLUSH, UI_GRID, a fresh
DMA_MESHLET, another DRAW, WAIT/FLUSH and END in one DRAM command buffer. It
checks retained 3D pixels before/after UI, binary transparency, the later draw,
and actual write terminals. Pin-write guards forbid writes outside the target;
full depth and immutable command/asset comparisons run at each checkpoint.
Separate disjoint layouts exercise the highest grid extent and atlas base.
Abort cases interrupt an in-flight UI write and the last output burst: accepted
work must drain while busy/fault remain asserted, without reaching the later
draw or reporting normal command-buffer completion.

`tests/ui_showcase.py` additionally takes an external WenQuanYi Bitmap Song font
zip, builds a shared atlas of ASCII, colored static icons and CJK halves, then
submits two command buffers without reset. Each CJK character occupies two
cells; Unicode and font rasterization exist only in the offline producer.
The second frame uses a different cell table and A/B target, verifies both
materialized color planes and unchanged depth, and exports PNGs from actual
SDRAM words. The producer reads WenQuanYi's original 100-unit bitmap grid
directly into 16x16 pixels (its 18px EM is not a 16px raster scale). ASCII uses
the native Windows Fixedsys 8x15 raster strike in an 8x16 cell. Neither path
resizes, antialiases or thresholds a vector-font rendering. Python Pillow,
fontTools and the Windows `vgafix.fon` resource are test prerequisites; font
binaries and rendered images remain generated artifacts.
