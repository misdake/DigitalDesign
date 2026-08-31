# CPU V3 Stage 10 benchmark and resource report

Date: 2026-08-31

## Benchmark method

The benchmark suite runs RCC-compiled programs on the cycle-accurate complete-system emulator after
Stages 9 and 10. Each workload is a separate source file under
`systems/cpu-v3-tang-nano-20k/benchmarks`, self-checks its result, and halts with signal 1 only on
success. The ignored release-mode tests run serially with an explicit cycle limit:

```text
cargo test --release -p cpu-v3-tang-nano-20k --test bench_emu runs_on_the_cycle_accurate_emu -- --ignored --nocapture --test-threads=1
```

The algorithms cover searching, sorting, prime generation, and dense integer matrix work. The game
proxies cover a tile-world update/scan, particle simulation, and a sprite batch writing a small
framebuffer-like surface. They model memory locality and control patterns rather than a particular
future GPU command format.

## Results

| Workload | Cycles | Retired words | Cycles / retired word | Fetch-wait cycles | Data requests | I-cache lines | D-cache lines | Redirects | Refreshes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Binary search | 364,311 | 146,476 | 2.487 | 46,152 | 11,782 | 7 | 146 | 23,044 | 604 |
| Matrix multiply, 16 x 16 | 1,461,643 | 604,714 | 2.417 | 182,409 | 34,620 | 10 | 49 | 91,169 | 2,423 |
| Particle update, 256 objects x 240 frames | 5,074,963 | 1,796,430 | 2.825 | 491,343 | 495,013 | 7 | 65 | 245,641 | 8,416 |
| Quicksort, 2,048 words | 2,063,228 | 718,765 | 2.871 | 184,597 | 217,423 | 12 | 478 | 92,247 | 3,421 |
| Sieve to 2,000 | 167,409 | 65,582 | 2.553 | 24,694 | 5,073 | 6 | 126 | 12,319 | 277 |
| Sprite batch, 64 x 64 writes x 60 frames | 8,994,716 | 3,553,096 | 2.532 | 998,689 | 245,760 | 5 | 28,336 | 499,321 | 14,912 |
| Tile-world scan, 1,200 cells x 180 frames | 6,562,160 | 2,626,699 | 2.498 | 873,158 | 217,380 | 7 | 75 | 436,546 | 10,882 |

All seven self-checks pass. The sprite proxy deliberately produces the worst cache behavior: its
wrapped, moving 64-word batches touch 28,336 D-cache lines. Particle and tile scans issue many loads
and stores but remain highly resident, producing only 65 and 75 D-cache line transactions. Quicksort
remains the most branch-heavy general algorithm and records 92,247 redirects. Prefetch is mostly
nominated and cancelled on these compact, control-heavy programs; the particle case is the only new
case that both issues and consumes a prefetch.

## Fitted resource and timing result

The post-Stage-10 Gowin build uses 9,977 Logic units: 8,766 LUTs, 683 ALUs, and 88 SSRAM cells. It
uses 4 DPB, 1 SDPB, 2 pROM, 2 MULT18X18, and 4,275 registers. PnR closes the 54 MHz CPU clock at
54.692 MHz, the 108 MHz Controller HS clock at 184.536 MHz, and the 74.25 MHz pixel clock at
80.543 MHz, with zero setup and hold TNS. The CPU margin remains narrow.

The synthesis hierarchy attributes the largest local blocks as follows. Parent and child rows overlap,
so these figures describe ownership and hot areas; they must not be added to reconstruct the device
total.

| Block | Registers | LUTs | ALUs | Dedicated memory / DSP | Observation |
| --- | ---: | ---: | ---: | --- | --- |
| CPU core | 853 | 3,134 | 230 | 40 SSRAM, 1 pROM, 2 DSP | Largest compute block; the current CPU critical path terminates in core GPR write data. |
| D-cache | 380 | 1,879 | 24 | 24 tag SSRAM, 2 DPB | Largest cache/control block; includes the 128-bit dirty engine (128 registers, 154 LUTs). |
| I-cache engine | 298 | 755 | 24 | 24 tag SSRAM, 2 DPB | Direct four-beat refill removed its complete-line register buffer. |
| SDRAM adapter and display staging | 794 | 770 | 0 | none attributed | Holds arbitration state plus the small line-write and display-drain staging arrays. |
| Fetch queue | 390 | 336 | 99 | none attributed | Four-entry instruction and outstanding-request metadata. |
| Boot DMA engine | 206 | 329 | 147 | none | Boot-only copy and checksum datapath. |
| HDMI/display pipeline | 233 | 234 | 54 | 1 SDPB | Includes the dual-clock scan-line BSRAM and three TMDS encoders. |
| Vendor SDRAM controller | 156 | 212 | 15 | vendor hard/opaque logic excluded | Physical 108 MHz controller side. |

The cache conversion is visible in the fitted memory modes: the four cache blocks are DPBs, the
display line buffer is the single SDPB, and boot/FPU data occupy the two pROMs. No cache line buffer
is inferred as registers or BSRAM.

The display adapter keeps two 256-bit staging arrays in registers. This is intentional for this
revision: one buffer is a stable line-write snapshot used by the related-clock gearbox, while the
other is only four 64-bit entries and drains independently after SDRAM ownership is released. That
early release is the useful performance optimization: the testbench proves a CPU transaction can be
accepted before all eight display words have drained. Moving either tiny array into synchronous
SSRAM would add address/output timing and control states, while the current design already closes the
pixel and controller clocks comfortably and the CPU critical path is elsewhere. Revisit RAM16
mapping only if later GPU/display integration creates register pressure.

Physical-board validation of Controller HS read-valid phase and write-burst sampling was not run in
this report; simulation, source audit, and PnR cannot replace that vendor-controller observation.
