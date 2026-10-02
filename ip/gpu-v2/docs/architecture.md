# GPU v2 component architecture

The IP owns reusable GPU components and their internal composition. A final
board system owns physical memory mapping, CPU/device integration, clocks,
display and firmware. GPU v2 currently has no complete-system command ABI.

```text
ip/gpu-v2/
  Cargo.toml                   gpu-v2 library
  src/
    lib.rs
    system/                    future whole-GPU composition through component ports
    command_processor/         typed command guards and fixed event identities
    scratchpad/                four banks, DMA/core ports and region leases
    vertex/                    v6 decode, matrix transforms and seven-row output
    frontend/                  bounded command/DMA/vertex and separate source-capture control
    memory/                    GPU-owned burst transport; vendor adapters stay outside production IP
    framebuffer/               materialized ROP oracle and bounded bank/maintenance control
    triangle/                  owned transformed inputs and setup oracle only
    texture/                   oracle/count, universal periodic preparation and bounded cycle-MC cache/color composition
    lighting/
      ports.rs                 component input/output and context contracts
      sim/
        oracle.rs              configurable reference and stage goldens
        counted.rs             closed audited fixed-point implementation
        timed.rs               bounded capacity and static batch reservations
        binding.rs             checked DSP fusion and small-logic lowering
        periodic.rs            static modulo calendar and repeating-slot audit
        physical.rs            concrete DSP/ROM placement and retained-value audit
        stream.rs              bounded mixed-mode admission, CE and ordered commit
        adder.rs               ordinary/increment sites, including embedded cones
      datapath.rs              compiled audited calendars and numerical executor
      emu/                     independent CE/backpressure numerical cycle model
      rtl/                     synthesizable shared lanes, retiming and Gowin probe
  tests/support/               deterministic stimuli and comparison helpers
  examples/                    bounded component probes and report export
  web/                         standalone Rust WASM lighting review and controls
  docs/                        implemented contracts and validation boundaries
```

This is the ownership plan. Empty model modules reserve directions and do not
claim implemented behavior. A new component uses the same structure. Ports are
owned by that component, not by one global GPU struct. The internal system
connects ports, queues and context lifetimes when integration is defined.

Simulation has three stages: configurable oracle determines numerical needs;
counted matches a chosen format configuration bit for bit and records all work;
timed starts by binding capacities and verifying static batch reservations.
The periodic variant uses the generic resource scheduler and checks repeating
arithmetic calendars. Physical certificates additionally check DSP placement,
concrete ROM banks/replicas/ports and retained-value capacity. Lighting's
independent emu/RTL implement numerical streaming, CE and backpressure for a
single drained context. Fast and Compact select full/diffuse II2/II1 and II3/II2
respectively. Resource/system alternatives remain explicitly selected candidates;
the original numerical profile stays the default. The isolated probes have
fitted evidence; whole-GPU/system composition remains future work.
The earlier mixed-mode reservation stream uses the framework's context/FIFO/
CE/commit control tokens with separately evaluated counted arithmetic.
Frontend replays DMA payloads, scratchpad leases and the vertex issue ROM with
an independent trace audit. Its current serial sequencer acceptance rate is
reported separately from the resource lower bound of a periodic vertex body.
These reservation models are not numerical cycle executors or complete GPU runtimes.
The [SDRAM memory controller combination](sdram-memory-controller.md) belongs to
the Gowin vendor crate. GPU tests use it through a dev dependency, with a GPU-owned
MemoryPort adapter for the frontend oracle. Both fixed-average and configured-load
Rust services return real data. The existing timed frontend still uses its explicit
latency fixture; it has not become a numerical cycle executor for this combination.
Emulation and RTL remain independent verification paths.
Tests use stage goldens rather than treating audit success as an accuracy oracle.
The separate GPU-owned `memory::ports` burst transport provides explicit write
completion through a bounded direct-combination test adapter. The standalone
framebuffer maintenance model consumes that port; frontend and production GPU
composition remain future work.

Texture advances the vendor cycle MC through a thin GPU-owned RefillPort in
test composition. Its bounded cache/color machine executes actual refill beats,
bank captures, closed color kernels and ordered results. Universal periodic
preparation calendars use fixed resources, registered cuts, finite credits and
early context release; composition retains IDs through pixel commit. Static
primitive and prepared-Group4 baselines remain diagnostic comparisons. The
declared storage map still needs compaction and fitted timing/area validation;
the controller's arithmetic goldens are not independent numerical emulation.

Lighting starts at a single pixel and returns two scalar intensities, g and h.
Quad allocation, coverage, four-pixel grouping and branch joins belong to the
future composition. Final color, texture sampling and framebuffer are separate
components. Component calculations can be tested without a GPU system.

Web code calls the same Rust oracle through bare WASM exports. It owns controls
and presentation only; it does not reproduce the arithmetic in JavaScript. The
[lighting review](../web/README.md) keeps scene/algorithm/ideal calculations in
Rust and has a native/WASM bit comparison. The planned runtime-format audited
lane and a complete GPU browser adapter have not been implemented.
Implemented behavior and validation are in [lighting](lighting.md) and
[frontend](frontend.md). Its separate source-capture control prepares self-contained
inputs for the triangle oracle and decouples source release from final fan use.
It does not schedule triangle arithmetic or connect the command sequencer to setup.
The frontend does not consume triangle records or define
an encoded GPU command ABI; framebuffer/cache and final system
integration remain separate work.
The [triangle oracle](triangle.md) consumes the vertex component's owned output
records directly. It provides self-contained source fields and coverage fans;
it does not read live/released vertex slots or instantiate a timed triangle queue.
The [texture models](texture.md) implement independent reference sampling, a
closed counted datapath, a functional cache through MemoryPort, and bounded
timed cache/refill/color execution through the existing SDRAM Service. A staged
companion verifies numerical boundaries and context/credit control proposals;
independent cycle arithmetic and physical sampling throughput remain unverified.

The [framebuffer model](framebuffer.md) implements materialized depth/blend,
four-bank tile addressing, demand refill, dirty writeback and flush. Serial and
overlapped ROP control calendars share the same capacities; the latter assumes
two-cycle oracle arithmetic returns rather than certified arithmetic. Tests
compare complete byte images and guards through fixtures and the actual serial
MC adapter. This is not counted ROP arithmetic, RTL or complete render/display
ownership, and maintenance currently excludes simultaneous hit execution.
