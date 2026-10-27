---
id: gpu-v2-sampling-throughput
status: implemented
last-verified: 2026-10-27
---

# Sampling throughput and latency

This chapter models the selected command GPU. Storage and ownership are in
[Execution](/ip/gpu-v2/docs/execution.md); the bounded regressions and their
entry points are in [Tests](/ip/gpu-v2/tests/README.md). Rates below count enabled
clocks unless wall clocks are explicitly stated. A stopped CE or external
backpressure adds wall time.

## Units and measurement boundaries

| Unit | Meaning |
| --- | --- |
| Covered pixel | One actual lane selected by the final numerical coverage mask |
| Quad | Up to four covered pixels; an empty quad issues no sample work |
| Mip member | One active fine or coarse filtering footprint for that pixel |
| Group | Nonzero taps sharing one8x8 tile in one mip; not a pixel or quad |
| Admission II | Spacing between accepted inputs while ready remains asserted |
| Completion II | Spacing between completed outputs under steady load |
| First latency | Accepted first input to its completed output, including stated cold work |
| Drain | Last accepted input to last output; includes queued accepted work |

A latency of34clocks for a quad can coexist with a completed quad every16clocks.
Four clocks per covered pixel follows from16clocks per quad only for a full mask.
Half-full quads at that same quad rate cost8clocks per useful pixel upstream.
Helpers have no UV reads, coordinate issues or sampler membership; removing
helper sample work does not make all upstream quad costs proportional to mask.

## Fixed local calendar

The coordinate inlet consumes U then V, accepting a covered lane every two
enabled edges. UV/descriptor Q stays owned through V. Coefficients select only
active mip members; membership splits each into its nonzero tile Groups.
The48-credit FIFO counts unpublished writes, stored entries, one in-flight read,
the copied current head and registered native next Q.

For the native Group read, let A be the first published entry:

| Edge | RAM read/return | Copied head | Native output Q |
| --- | --- | --- | --- |
| 0 | Read A into the BSRAM first stage | Empty | Invalid |
| 1 | OCE returns A | Empty | A becomes valid |
| 2 | Read B; promote A | A | A held, now unowned |
| 3 | OCE returns B | A held | B becomes valid |
| 4 | Consume A, promote B, read C | B | B held; its key also feeds tag lookup |
| 5 | OCE returns C | B held | C becomes valid |
| 6 | Consume B, promote C, read D | C | C held |

Q return and promotion are mutually exclusive. OCE is low on a promotion/tag
lookup edge; the live Q cannot change as consumers sample it. CE freezes this
calendar. Prefix reads never depend on a same-edge producer write. This uses
the BSRAM output register, not an additional wide fabric register bank.

The cache has a separate paid-address slot and held result. It can prepare the
next texel read while Color consumes the old result. The next bank read occurs
on the old result's final consume edge. Paid admission to synchronous bank return
has two-edge startup, while steady cache/Color completion is one Group per two
clocks. Queue prime, tag prime, coordinate latency and Color completion still
exist; this is not a two-clock input-pixel-to-output-pixel latency claim.

## Resident work and local capacity

Count only Groups containing nonzero weighted taps. With all four bilinear taps
active, an interior, X/Y edge or XY corner footprint costs one, two or four Groups.
True fractional trilinear performs this work for both active mip levels. Filter
mode2 with zero coarse weight is single-mip work; its name alone proves nothing.

| Resident workload | Groups/pixel | Saturated local clocks/pixel | Samples/s at54MHz |
| --- | ---: | ---: | ---: |
| Single-mip interior | 1 | 2 | 27,000,000 |
| Single-mip every pixel at an X/Y tile edge | 2 | 4 | 13,500,000 |
| Single-mip every pixel at an XY tile corner | 4 | 8 | 6,750,000 |
| Two mips, both footprints interior | 2 | 4 | 13,500,000 |
| Two mips, one footprint at an X/Y edge | 3 | 6 | 9,000,000 |
| Two mips, one footprint at an XY corner | 5 | 10 | 5,400,000 |
| Two mips, both footprints at XY corners | 8 | 16 | 3,375,000 |

The sampler floor is `max(2, 2*mean_Groups_per_pixel)`. Coordinate admission
supplies the first term; Group service supplies the second. Finite queues and
owner limits add transients. Tap weights quantized to zero can reduce work.
Fine/coarse coordinates are related, so a concrete UV must be checked before
claiming a particular pair of crossing patterns.

For uniform independent X/Y footprint origins within a tile, each mip averages
`(1+1/8)^2 = 1.265625` Groups when all taps remain active. Two active mips therefore
need about5.0625clocks/pixel. Linearity permits summing the mip expectations;
fine and coarse crossings need not be independent of one another. This is a
phase-distribution estimate, not a guarantee for arbitrary UV gradients or wraps.

## Why buffering can retain four clocks per pixel

For75%interior plus25%Xedge single-mip pixels, mean local work is2.5clocks/pixel.
For75%interior plus25%XYcorner it is3.5. Both fit a producer offering one full quad
every16clocks: the queue absorbs the temporarily expensive lane and catches up
on the cheaper lanes. The nonuniform-texture regression requires exactly that
steady quad rate for bilinear and for trilinear genuinely reduced to one mip.

For two active mips, interior work already consumes the4clocks/pixel budget.
Sustained crossings can demand more than that budget. A queue can hide finite
bursts, but cannot hide a permanent service-rate deficit. Compare warm occupancy
with the available credits; cold miss accumulation and steady burst occupancy
are different measurements.

## Misses, startup and whole-GPU throughput

The Group/cache regression uses a fixed16-beat64-bit memory peer and requires
20clocks between consecutive missed Groups. This is a cache-schedule contract,
not the real SDRAM latency. The physical MC adds command/row/refresh delays,
related-clock return transport and arbitration, including Display when active.
A cold first output and a warm steady window must be reported separately.

The tile regression uses512pixels, checks its warm window after256outputs and
reports first latency, warm intervals, last-input drain, cold/warm Group peaks,
actual lambda and fine/coarse member counts. Its nonuniform RGB565 sector colors
and integer address/weight reference detect wrong tiles and mip selection; white
texels alone would conceal many such faults. Complete quad arrival spacing is
separate from the latency of its last result.

A whole-GPU rate includes V/T, bounds/tile traversal, interpolation, useful mask
population, sampling/lighting, Final/ROP, DMA, refill/writeback and drain. A useful
lower bound is the largest sustained stage work, plus only unhidden startup and
inter-stage gaps; adding every busy counter double-counts overlapping work.
Meshlet/draw setup can dominate small triangles even when hit sampling is fast.

Board submission intervals include CPU SEND/completion observation and real
binding/DMA prologues. CLEAR and untimed cache setup need explicit accounting.
Headless intervals precede the first Display present; later visible previews
exercise concurrency but are not headless performance samples. A1us timer value
converted with54clocks/us is an approximate cycle count, not an internal exact
cycle counter. Retain individual samples, resolution, saturation checks and the
stated cold/warm policy. Repeated identical samples do not remove timer error.

Neither static timing closure nor ideal RTL simulation establishes board image
correctness. Qualification binds the actual source, constraints, complete timing
report, bitstream, boot program and resources; it also checks repeated redraws.
A lower-frequency diagnostic cannot qualify the normal54/108MHz implementation.
