# GPU raster and framebuffer control comparison

## Measured candidates, 2026-09-27

The preferred configurations are **tile traversal with 2 pixels/clock** for throughput
and **tile traversal with 1 pixel/clock** when the smaller logic budget matters. The
default remains the two-pixel producer. These rates describe coverage generation:
one 2x2 quad every two or four clocks in the raster loop. The current pixel adapter
still emits at most one covered pixel per clock, and the framebuffer cache still
blocks during refill/clean. This experiment does not implement a complete pixel pipeline.

All four candidates use the same full-system export, firmware, 54/108-MHz constraints,
Gowin Education 1.9.8.11, C8/I7 slow corner, and place/route algorithms 1. Only
`PIXELS_PER_CYCLE` and `SCANLINE` differ; the integrated acquisition limit is K=1.
The comparison uses the dirty optimization tree based on `11729ac`, with generated
source, pin and clock hashes captured for every candidate. Fits are isolated clones;
their edited exports are experimental artifacts and cannot be programmed through a
production manifest. The audited default build has its own validation boundary in
[architecture.md](architecture.md#current-fitted-result-and-validation-boundary).

Raster ownership below includes the serial-pixel wrapper. Its LUT/ALU/FF figures
come from the integrated synthesis hierarchy; **Logic = LUT + ALU + 6 x RAM16**.
Whole-system Logic and FF are final PnR results. Hierarchy totals are attribution,
not independently placed modules, and cannot be summed to reproduce PnR totals.

| Traversal | Pixels/clock | Raster LUT | Raster ALU | Raster Logic | Raster FF | System Logic | System FF | CPU Fmax MHz | Worst setup ns |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Tile | 1 | 1,535 | 498 | 2,033 | 1,067 | 13,924 | 5,754 | 55.631 | +0.543 |
| Tile | 2 | 1,540 | 585 | 2,125 | 1,066 | 14,042 | 5,753 | 55.665 | +0.554 |
| Scanline | 1 | 1,818 | 618 | 2,436 | 1,103 | 14,513 | 5,792 | 58.009 | +1.280 |
| Scanline | 2 | 1,852 | 704 | 2,556 | 1,102 | 14,472 | 5,792 | 56.660 | +0.869 |

Each raster uses one BSRAM and four synthesized DSP resources, with no RAM16 at K=1.
Each system uses 98 RAM16 cells and the same 13 BSRAM blocks. All four final candidates
pass setup and hold. Against the preceding B-source/AABB fit, the default system
reduces Logic by 1,692 and FF by 1,969. Integrated GPU synthesis ownership falls from
5,140 to 3,566 Logic: raster/wrapper 3,467 to 2,125, and the remaining GPU control
1,673 to 1,441. The latter includes command/list decode and bank control, not just
cache metadata. Different synthesis/PnR accounting explains why these deltas do not add exactly.

## Producer throughput

`viewport_raster_candidate_matrix` checks 40 deterministic scenes in all four modes,
under three sink/acquisition stall patterns. Quad coordinate, mask, multiplicity and
triangle identity match the independent reference; retirement markers retain order.
The corpus includes rejected winding, zero area, subpixel/shared edges, all screen
borders, a guard-band example and twelve seeded triangles. Metrics count internal
quad enqueue events, so FIFO buffering cannot masquerade as a faster producer.

The unthrottled quad sink still uses the fixture's 6/7 acquire-ready pattern. Total
cycles include setup, empty regions, tile transitions and marker completion. The
weighted mean below is total cycles divided by 99,494 covered quads, including the
cycles of zero-coverage scenes. A zero-coverage scene has no individual cycles/quad value.

| Traversal | Pixels/clock | Dense-loop peak clocks/quad | All 40 scenes, clocks/covered quad | Nonempty scenes only |
| --- | ---: | ---: | ---: | ---: |
| Tile | 1 | 4 | 5.040 | 4.998 |
| Tile | 2 | 2 | 2.611 | 2.588 |
| Scanline | 1 | 4 | 5.044 | 5.012 |
| Scanline | 2 | 2 | 2.926 | 2.902 |

Both two-pixel implementations meet the requested peak and measured average targets.
These are workload measurements, not an upper bound for arbitrary triangles: tiny
triangles pay setup, and arbitrarily thin or rejected inputs can yield no quad.
The one-pixel choices sustain four clocks per visited quad in the inner loop, but
do not meet a four-clock average when setup and empty work are included.

| Producer scene | Covered quads | Tile 1 cycles | Tile 2 cycles | Scanline 1 cycles | Scanline 2 cycles |
| --- | ---: | ---: | ---: | ---: | ---: |
| Steep diagonal | 864 | 5,304 | 2,743 | 4,948 | 2,974 |
| Crossing viewport | 5,671 | 25,172 | 13,104 | 29,123 | 17,051 |
| Shared edge | 1,540 | 8,668 | 4,532 | 8,442 | 4,932 |
| Screen borders | 24,160 | 108,284 | 55,805 | 116,061 | 66,674 |
| Guard band | 22,170 | 93,479 | 48,167 | 95,200 | 50,738 |

The tile two-pixel producer costs only 92 more raster Logic and 118 more system
Logic than tile one-pixel, while reducing corpus cycles by about 48%. Its covered
quad throughput per raster Logic is about 85% higher. This is the recommended default;
the one-pixel tile mode is the explicit smaller alternative, not a claim that
two-pixel throughput is too expensive or infeasible.

## Cache integration and scanline locality

The scanline experiment traverses two-row bands, tests the left span using the same
two edge DSP products used for initialization, and stops at a proven empty right
tail. Prefix skips occur in eight-quad chunks; row transitions retain exact edge
accumulators. It emits the same covered pixels and markers as tile traversal, but
can reacquire a tile on several bands. Scanline qualification assumes clipped and
snapped vertices whose sample-to-vertex subtraction does not wrap inside the viewport.
The guard-band test is covered; arbitrary extreme raw s12.4 inputs are not a
qualification of this experimental traversal. Default tile behavior remains unchanged.

`gpu_raster_candidate_images_and_throughput` compares all 26 full 400x240 images
against an independent integer pixel-center/top-left oracle, including nonzero
LOAD contents and payload guards. Every mode also checks all 36 injected read/write
error positions. Integrated cycles include END cleaning and serial pixel/cache stalls.

| Scene | Tile 1 cycles | Tile 2 cycles | Scanline 1 cycles | Scanline 2 cycles | Tile read/write requests | Scanline read/write requests |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Shared edge | 2,365 | 2,275 | 3,813 | 3,553 | 24 / 24 | 56 / 24 |
| Screen edges | 4,056 | 3,848 | 5,251 | 4,980 | 40 / 40 | 64 / 40 |
| Alias eviction | 14,018 | 13,567 | 21,973 | 21,422 | 140 / 140 | 292 / 268 |
| Wide triangle | 88,092 | 87,355 | 302,805 | 302,025 | 820 / 820 | 5,248 / 5,208 |

Each framebuffer request transfers 128 bytes. The row-major sweep revisits more
tiles than the eight-entry direct-mapped cache can hold; dirty evictions and LOAD
refills dominate the wide example. Two-pixel scanline needs 431 more raster Logic
than two-pixel tile and also has worse cache locality. Retain it as a tested
experiment; select tile at both rates for the current tile-linear framebuffer.
A cache-local strip traversal is a separate candidate, not a measured result here.

## Changes retained and alternatives rejected

- Viewport edge accumulators use 33 signed bits; full geometry retains 40. Wrapped
  16-bit coordinate deltas and coefficients bound initialization by 2^31, and
  traversal over 400x240 adds less than 2^29, safely within signed 33-bit range.
  Top-left tests and unbiased edge values remain exact; no depth/varying precision
  is traded away.
- A viewport triangle owns one setup record until marker ACK. Tile jobs therefore
  store only band/tile coordinates and index, reconstructing their rectangles from
  the locked record. The full-geometry setup/fan queues and independent records
  remain. K=1 uses a single job register; K=2/3 keep the distributed job queue.
- One-pixel traversal checks lanes 00,01,10,11, sharing `dx-dy` in both odd phases.
  A controlled fit reduced raster Logic by 198 and system Logic by 431 versus the
  prior one-pixel lane order. Two-pixel traversal checks adjacent lanes in two
  clocks; the former four-pixel/clock and unused commit state are removed.
- Even quad origins and outward odd maxima make all four lanes belong to the
  clipped rectangle. Assertions check this invariant. Per-lane rectangle compares
  are unnecessary, and the pixel adapter appends the lane bit to each coordinate.
- The quad/marker FIFO explicitly requests BSRAM. After coordinate consumers were
  narrowed, automatic mapping changed to FFs: forcing BSRAM saved 444 system Logic
  and 886 FF in a paired fit. A distributed-RAM attribute alone did not produce
  RAM16. This observed mapping change explains a real regression after simplification;
  the tool's exact automatic-selection heuristic is not established.
- A more aggressive alias of setup scratch and the private tile-stage record saved
  242 FF but added 25 system Logic and 31 raster Logic in paired BSRAM fits. Keep
  the private record. FF count alone is not the acceptance metric.
- Moving all small FIFOs to RAM16 added 89 cells, costing 534 Logic before mux/control
  effects, while saving only 216 system Logic in that earlier fit. It is inferior
  to compact ownership records and is not retained.
- GPU command payloads retain consumed fields plus a predicate for each reserved
  high-bit range. All 143 individual reserved-bit cases still reject. Sharing the
  eight-entry tag read port saves two RAM16 cells; valid/dirty reset still makes
  stale tags unobservable, verified by reset followed by LOAD under a new target.
- Command and tile-list lines occupy separate four-beat halves of one eight-deep,
  64-bit RAM16 array. The serial FSM shares one asynchronous read and one fill port,
  retaining both lines and the complete public command contract. This saves sixteen
  RAM16 cells; a paired full-system fit saves 181 Logic. Existing multi-command,
  cache/error and boundary traces exercise preservation across list fetches.
- Command qword counters use the maximum aligned 16-bit submission extent; list
  bounds use the 22-bit physical memory contract plus explicit high-bit rejection.
  The 65,532-word maximum stream, terminal payload overrun, and each address bit
  22..31 are tested, including the existing legal-empty-list semantics.
- Cache memory-port reads are enabled only during clean. This removes the address
  mux formerly used to avoid inactive-port collisions; the simulator checks actual
  simultaneous enabled access. Four independent physical render banks remain.

## Whole-system ownership and timing

Synthesis ownership in the same tile two-pixel candidate identifies the remaining
large consumers. These values are not added to the PnR total.

| Owner | Logic | Explanation / disposition |
| --- | ---: | --- |
| CPU core | 4,077 | FPU contributes 2,166; scalar control, register bypass and lane dispatch remain required. |
| GPU | 3,566 | Raster/wrapper and command/cache control are detailed above. |
| D-cache | 1,409 | Write-back/tag/maintenance control; dirty leaf is 157, tags 192, valid/victim leaf 100. |
| Shared SDRAM adapter | 815 | Ordered variable-length transactions and 64-bit staging remain required. |
| Fetch queue/BTC | 808 | Four slots, redirect handling and two-word BTC are required by the CPU pipeline. |
| Display | 668 | TMDS, scanout and CDC support the real display load. |
| I-cache | 565 | Resident lookup, tags and invalidation. |
| Memory arbiter | 495 | Seven owners, display priority, age and fairness. |
| Boot DMA | 480 | Generated package verification and loading. |
| Flash reader | 238 | Existing boot path. |
| SDRAM controller | 230 | Vendor controller boundary. |
| System control | 81 | Cache-maintenance and device contracts. |
| Write gearbox | 51 | Ordered 108/54-MHz write transfer. |

The D-cache dirty bitmap is deliberately still FF storage: it supplies a sixteen-entry
maintenance window and global dirty status while accepting dirty updates. A simple
single-address RAM16 replacement would change these accesses and require summary or
pipeline work. Its small measured ownership does not justify an unrelated cache rewrite.
No other inspected owner showed a confirmed large, unnecessary storage structure.

Smaller Logic does not guarantee faster routing. The two-pixel tile candidate is
limited by fetch-queue head selection through BTC replacement-rank write enable,
not raster or framebuffer arithmetic: 19 logic levels, 7.633 ns cell, 10.064 ns
route, plus 0.232 ns clock-to-Q. The one-pixel tile path is I-cache pending address
to data-bank read address. Scanline one-pixel is instruction decode to BTC rank;
scanline two-pixel is fetch-queue head to FPU word write enable. Earlier intermediate
scanline fits failed 54 MHz on fetch/cache paths despite reduced raster logic;
the final four fits all pass. Placement, packing, fanout and cross-module sharing
change with the whole netlist. No clock reduction or false-path exception is used.

Occupied CLS falls by 542, less than the Logic-equivalent reduction of 1,692;
packing constraints and RAM/ALU accounting matter as well as the cell count.
The preceding first D-cache path had 15 levels, 6.292 ns cell and 10.337 ns route.
The new first path has more cell delay but slightly less aggregate route delay,
and belongs to a different owner. The Fmax reduction alone therefore does not
establish worsening wire delay on the former path. Both fits retain 54-MHz runtime.

## Full-GPU boundary and deferred prefetch

Keep the four true-dual-port cache banks and swizzle, complete LOAD/CLEAR behavior,
dirty cleaning and error completion, epoch/triangle marker ACK, full-geometry
clip/fan/setup source, and three independent GPU memory masters. These support
known future depth, varying and pixel processing requirements. K=1 single-job and
record sharing specialize today's serial viewport path; they do not replace the
full-geometry queues or promise multi-triangle overlap. Concurrent refill/render
will require explicit entry ownership and arbitration before reuse of today's FSM.

The cache already receives K=1 acquire requests. **Earlier framebuffer prefetch to
hide future pixel-processing latency is recorded, not implemented in this change.**
Its later design must retain pin/hazard, LOAD/CLEAR, draw/epoch, dirty-victim and
error semantics. No speculative second cleaner/tag port or extra in-flight buffering
is paid for before that schedule is defined.

## Reproduction and evidence boundary

```powershell
scripts/measure-gpu-raster.ps1 -Jobs 4 -GowinHome $env:GOWIN_HOME
scripts/run-cargo.ps1 -Subcommand test -Label raster-matrix -CargoArgs @(
  '-p','cpu-v3-tang-nano-20k','--lib','rasteremu::rtl_tests','--',
  '--ignored','--nocapture','--test-threads=1')
scripts/run-cargo.ps1 -Subcommand test -Label gpu-integration -CargoArgs @(
  '-p','cpu-v3-tang-nano-20k','--test','gpu_trace_cosim','--',
  '--ignored','--nocapture','--test-threads=1')
```

The fit script captures source/constraint hashes, writes candidate and module-owner
CSVs, and fails if a candidate violates timing. The tests emit producer and cache
throughput CSVs and fail on image, ordering, conservation or error-path divergence.
Six raster RTL tests, eight GPU integration tests, both mandatory CPU/system co-sims,
full Flash simulation and aggregate hardware/artifact checks are required for promotion.
Simulation, PnR, SRAM loading, complete Flash Verify, cold boot and HDMI observation
are separate evidence. The current board/image boundary belongs only to the
[architecture validation section](architecture.md#current-fitted-result-and-validation-boundary).
