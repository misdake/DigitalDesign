# Gowin device references

Reusable primitive construction, port timing, and measured resource rules.
Device/tool versions and validation boundaries are recorded in each document.

| Document | Purpose |
| --- | --- |
| [tang-nano-20k-clock-constraints.md](tang-nano-20k-clock-constraints.md) | PLL/SDC agreement, optional 49.5/99 MHz parameters, total setup/hold uncertainty and delayed DQ monitoring. |
| [gowin-bsram-timing.md](gowin-bsram-timing.md) | BSRAM geometry, SDP/TDP ports, edge timing, CE/OCE, writes, collisions, and reset. |
| [gowin-dsp-coexistence.md](gowin-dsp-coexistence.md) | Measured GW2AR-18 DSP macro packing, fused primitives, control sets, and budget counting. |
| [sdram-memory-controller.md](sdram-memory-controller.md) | Shared arbiter/adapter/gearbox/controller ownership, fixed stream export, independent data reference and verification boundaries. |

Raw experiments and their reproduction scripts remain in `D:/fpga/experiments/`.
