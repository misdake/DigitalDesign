# CPU V3 Stage 10 resource and timing report

Date: 2026-08-31

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
