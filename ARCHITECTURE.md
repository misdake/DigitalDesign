# DigitalDesign architecture

The workspace is organized by technical layer rather than by project age:

```text
circuit/                         gate graph, simulation, Verilog rendering
hardware/
  core/                          hardware description and project/resource APIs
  macros/                        hardware derive macros
  common/                        vendor-independent FPGA shell components
  vendor/gowin/                  Gowin tools, primitives, and board targets
ip/
  common/                        physical-memory and device-channel contracts
  cpu-v1/                        reusable CPU V1 processor IP
  cpu-v2/                        CPU V2 ISA, model, and RCC backend
  cpu-v3/                        CPU V3 ISA, model, Gowin-bound cache/RTL, and RCC backend
compiler/
  rcc/                           frontend, target-independent IR and passes
  isa-macros/                    ISA definition macros
  tools/                         multi-target compiler tools
systems/
  cpu-v1-sim/                    CPU V1 memory, devices, display, and programs
  cpu-v2-sim/                    CPU V2 runner and debugger
  cpu-v3-tang-nano-20k/          fitted FPGA system and boot chain
```

## Ownership rules

- `circuit` has no knowledge of hardware targets, processors, compilers, or systems.
- `hardware/core` describes modules, projects, resources, and tests. Vendor APIs live below
  `hardware/vendor`, and concrete board integration never moves into `hardware/core`.
- `ip` crates expose reusable, narrow ports. They do not receive a complete system memory map as
  a generic type parameter.
- `compiler/rcc` owns parsing, validation, target-independent IR, optimization, allocation
  machinery, and source debug data. CPU ABI, legalization, encoding, linking, and target debug
  finalization belong to each CPU's `rcc_backend`.
- A `system` is the final composition boundary. It owns fitted memory/device layouts, board-level
  top modules, host tools used only by that system, firmware, applications, and boot packaging.

Dependencies point upward through this document: a later layer may use an earlier layer, never the
reverse. In particular, `compiler/rcc` cannot depend on a CPU and `hardware/core` cannot depend on
a processor or a system.

`scripts/check-layering.ps1` derives these layers from workspace manifest paths, so future
`ip/gpu-*`, `ip/audio-*`, vendor targets, and systems are covered without maintaining a crate-name
allowlist. A final system may compose any lower layer but may not depend on another final system.

## Physical memory and devices

CPU V2/V3 systems use physical addresses only. CPU V3 forms a 32-bit word address by concatenating
`segment` and `offset`; the target adapter validates and narrows it to fitted memory capacity.
`digital-design-ip-common` defines `SystemMemoryLayout`, validated non-overlapping regions, the
16-bit-word ready/valid single-outstanding memory contract, two-byte write masks, and system-owned
device-channel allocation.

`dev_send`/`dev_recv` are the control plane. Shared physical memory is the CPU/GPU/DMA data plane.
The CPU cache is write-through. CPU-to-GPU ownership transfer waits for completed stores; GPU
completion drains its writes; CPU ownership resumes only after the system-control device completes
a D-cache invalidation. There is no MMU, snooping, burst protocol, transaction ID, or multiple
outstanding request support yet.

## CPU V1 pilot review

The pilot produced two crates without changing its Harvard memory semantics. `ip/cpu-v1` owns the
ISA, assembler, core, reference model, and abstract device bus. `systems/cpu-v1-sim` owns concrete
memory devices, display/gamepad integration, programs, and Sokoban. This confirmed that the useful
boundary is a small port trait; making the CPU generic over a full system layout would couple it to
irrelevant policy. Tests follow the owner of the behavior: core/reference tests stay with the IP,
while device and complete-program tests stay with the system.

## Stability policy

Directory migration must not change ISA encodings, compiler listings, boot bytes, generated
Verilog identities, resource claims, or simulator results. The CPU V3 system build script compiles
Stage0, Stage1, and both boot demos from their real RCC sources and writes generated arrays only to
Cargo `OUT_DIR`; no checked-in byte array is maintained in parallel.
The build retains FNV-1a baselines for the 590-word Stage0 image and the 2,563-byte
Flash package, so generation remains single-source without weakening byte-for-byte compatibility.

## Board bring-up and diagnostics

Reusable board control belongs in `hardware/common`, not in processor or memory test harnesses.
`ResetController` synchronizes an external reset and clock-ready indication, then holds reset for a
specialization-defined number of complete clock cycles. A system chooses that interval; an IP only
receives the resulting active-high reset.

`DiagnosticReporter` owns serialization of the eight-byte DDHT v1 status frame. A test harness
provides only `report_enable` and an atomically sampled status byte. This keeps the UART protocol
out of CPU, SDRAM, BSRAM, DMA, and future GPU/audio self-tests. A protocol transmitter remains
inside a harness when its bytes are themselves the behavior under test, such as the system-control
UART and the SDRAM word-port `SDWP` signature.

The vendor-level `board_health` probe is the first hardware gate. It deliberately avoids CPU,
memory, PLL, Flash, and system-control dependencies. Higher-level hardware evidence is meaningful
only after the exact built artifact passes manifest validation and this transport probe produces a
valid frame.
