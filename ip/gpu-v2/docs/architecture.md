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
    frontend/                  bounded command/DMA/vertex composition
    triangle/                  owned transformed inputs and setup oracle only
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
      emu/                     future independent cycle/state model
      rtl/                     future hardware implementation
  tests/support/               deterministic stimuli and comparison helpers
  examples/                    bounded component probes and report export
  web/                         future WASM display/control adapter
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
concrete ROM banks/replicas/ports and retained-value capacity. Lighting connects
the framework's context/FIFO/CE/commit control tokens to a bounded mixed-mode
reservation stream; payload arithmetic is separately evaluated by counted.
Frontend replays DMA payloads, scratchpad leases and the vertex issue ROM with
an independent trace audit. Its current serial sequencer acceptance rate is
reported separately from the resource lower bound of a periodic vertex body.
Neither reservation model is a numerical cycle executor or complete GPU runtime.
Emulation and RTL remain independent verification paths.
Tests use stage goldens rather than treating audit success as an accuracy oracle.

Lighting starts at a single pixel and returns two scalar intensities, g and h.
Quad allocation, coverage, four-pixel grouping and branch joins belong to the
future composition. Final color, texture sampling and framebuffer are separate
components. Component calculations can be tested without a GPU system.

Web code will call the same Rust numerical model through WASM. It owns controls
and presentation only; it must not reproduce the arithmetic in JavaScript. The
runtime-format audited lane and the WASM interface have not been implemented.
Implemented behavior and validation are in [lighting](lighting.md) and
[frontend](frontend.md). The frontend does not consume triangle records or define
an encoded GPU command ABI; framebuffer/cache, sampling and final system
integration remain separate work.
The [triangle oracle](triangle.md) consumes the vertex component's owned output
records directly. It provides self-contained source fields and coverage fans;
it does not read live/released vertex slots or instantiate a timed triangle queue.
