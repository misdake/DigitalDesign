# Gowin device references

Reusable primitive construction, port timing, and measured resource rules.
Device/tool versions and validation boundaries are recorded in each document.

| Document | Purpose |
| --- | --- |
| [gowin-bsram-timing.md](gowin-bsram-timing.md) | BSRAM geometry, SDP/TDP ports, edge timing, CE/OCE, writes, collisions, and reset. |
| [gowin-dsp-coexistence.md](gowin-dsp-coexistence.md) | Measured GW2AR-18 DSP macro packing, fused primitives, control sets, and budget counting. |
| [sdram-memory-controller.md](sdram-memory-controller.md) | Shared arbiter/adapter/gearbox/controller ownership, Rust oracle service, load configuration and verification boundaries. |
| [sdram-traffic-probe.md](sdram-traffic-probe.md) | Standalone 54/108 MHz CPU/display/GPU probes, serial/early/group comparisons, fitted resource scope and UART capture. |

Raw experiments and their reproduction scripts remain in `D:/fpga/experiments/`.
