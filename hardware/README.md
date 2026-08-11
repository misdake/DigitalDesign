# Digital Design Hardware

`digital-design-hardware` adds named, hierarchical hardware modules above the
wire/NAND simulator in `digital-design-code`. A module keeps its emulation,
NAND, and Verilog contracts on one Rust type while allowing a machine design
to mix emulated and NAND implementations explicitly.

## Module shape

Input and output bundles derive `ModuleIo`:

```rust
#[derive(Clone, ModuleIo)]
pub struct CounterInput {
    pub reset: Wire,
    pub enable: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CounterOutput {
    pub count: Wires<6>,
}
```

The derive generates `CounterInputValue` and `CounterOutputValue` for tests.
`Wires<N>` becomes a packed Verilog bus with `wires[0]` as the least
significant bit.

Implement `Module` on one type:

- `create_emu`, `execute_emu`, and `clock_emu` implement the external-backed
  cycle model;
- `nand` builds the primitive circuit and may remain unimplemented during
  development, in which case it fails immediately;
- `build_verilog` defaults to `nand`, or may call child `Module::verilog`
  methods to preserve hierarchy;
- `verilog_source` may include a complete adjacent Verilog-2001 module. Its
  ANSI port signature is checked against the Rust IO contract;
- const-generic modules should override `verilog_name` with a stable name that
  includes every parameter that changes their generated hardware.

Generated NAND registers use the project main clock. Additional clocks of an
external hardware block are ordinary input ports; its emulator is responsible
for detecting and applying those clock edges.

## Cycle tests

`ModuleTest` runs identical typed vectors against independent emulated and
NAND circuits. The NAND circuit is first checked to contain only gates and
registers, so accidentally calling a child module's `emu` implementation is a
test failure. Each step has one fixed sampling rule:

```text
drive inputs -> settle -> main clock edge -> settle -> sample outputs
```

This rule makes register latency and reset/enable priority observable. A
hardware-specific emulator must only model timing and collision behavior that
is supported by documentation or measurements. Uncalibrated RAM collisions,
CDC behavior, and initialization must remain explicit unknown/error cases
rather than silently selecting a convenient value.

Board tests are a separate layer. Hardware test RTL should expose checked
cycle counts, sticky errors, and first-error context. Larger tests should emit
framed UART telemetry so a capture can be decoded and replayed as emulator
conformance vectors.

## Hardware targets and resources

A project selects one complete, purchasable hardware variant as a Rust type.
The target directly combines its backend, exact Gowin selection fields,
fabric capacity, fitted memory, and board-level components:

```rust
type Target = TangNano20K;
let mut project = GowinProject::<Target>::new("counter", "board_top");
```

There is deliberately no separate FPGA-model type: repeating chip capacities
for a second board revision is simpler and less ambiguous than merging device,
package, stepping, memory-fit, and dock identities. A target implements
`Supports<C>` only for component families it provides; the resource allocator
checks the requested quantity:

```rust
project.take_named("main-clock", Clock27M)?;
project.take_named("leds", UserLeds::<6>)?;
project.take_named("frame-buffers", BsramBlocks::new(4))?;
project.take_named("geometry", DspMultipliers::new(2))?;
```

Every `TargetComponent` returns a list of lower-level `ResourceAmount`
requirements. A take is transactional across the whole list. BSRAM reserves
18-Kbit blocks and DSP reserves 18x18 multiplier capacity, which are common to
the currently modeled Gowin variants. SDR SDRAM and DDR3 remain different
resource kinds. SDR SDRAM, DDR3, SPI flash, and SSRAM capacities are accounted
in individual bits using `u64`; `from_mibits()` is only an ergonomic input
conversion. Later allocators can add width modes, ports, packing, and
alternative implementations behind this component boundary. Any failed take
poisons the allocator: later takes and project export fail with the original
reason, so a capacity error cannot be ignored accidentally. Exported Gowin
projects include `resource-report.txt`; synthesis and place-and-route remain
the final authority.

Resource leases are also required for physical IO binding. Tang Nano 20K
provides exact clock, button, and active-low LED pin definitions:

```rust
let clock = project.take_named("main-clock", Clock27M)?;
let buttons = project.take_named("buttons", UserButtons::<2>)?;
let leds = project.take_named("leds", UserLeds::<6>)?;
let binding = TangNano20K::bind_user_io(clock, buttons, leds, "buttons", "leds");
let project = project.with_board_binding(binding);
```

The binding generates the board wrapper, CST, and SDC files. Export validates
the bound logic-port names, directions, and widths before invoking Gowin.

`TangConsole138KC128M` models Tang Console fitted with the current C-step,
128-Mbit-flash Mega 138K SOM. Its Gowin project identity is verified against
the installed Education 1.9.11.03 device database; a board wrapper and pin
constraints are still required for a useful physical design.

The initial target facts are intentionally small and auditable:

| Resource | Tang Nano 20K | Tang Console 138K C/128M |
| --- | ---: | ---: |
| LUT4 | 20,736 | 138,240 |
| Flip-flop | 15,552 | 138,240 |
| SSRAM (bits) | 41,472 | 1,105,920 |
| 18-Kbit BSRAM blocks | 46 | 340 |
| 18x18 multipliers | 48 | 298 |
| PLL | 2 | 12 |
| Fitted main memory | 64 Mibit SDR SDRAM | 8,192 Mibit DDR3 |
| SPI flash | 64 Mibit | 128 Mibit |
| User LED channels / keys | 6 / 2 | 3 / 2 |

Fabric resource counts follow the Gowin GW2AR/GW5AT family tables. Board and
variant facts follow the Sipeed Nano 20K and Tang Console documentation. The
Console reconfiguration key and power indicators are not application
resources. High-speed transceivers, PCIe, USB, and shared/multiplexed dock pins
are intentionally not modeled yet; adding them requires allocation groups and
pin-conflict information, not just another counter.

## Basic adder Gowin example

Export the Tang Nano 20K example:

```text
cargo run -p digital-design-hardware --example export_basic_adder -- target/basic_adder_gowin
```

The result contains generated hierarchical HDL, a board wrapper, CST, SDC,
Gowin project XML, and `build.tcl`. The testable `BasicAdderBoard` hardware
module synchronizes both buttons and produces a 4 Hz enable from the 27 MHz
clock. The generated binding connects it to pin 4, the two buttons, and the six
active-low LEDs. No hand-written board directory is used.

Build without programming the board:

```powershell
cargo run -p digital-design-hardware --example export_basic_adder -- --build
```

SRAM programming is intentionally an explicit operation and is never run by
`cargo test`. With a connected board it can be requested with `--program`;
this first performs a clean export and build, then programs volatile SRAM.
