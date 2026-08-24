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
- every module derives `Hardware`; the derive creates its explicit
  `VerilogIdentity` and includes every const generic automatically. Each
  concrete Rust specialization becomes one concrete Verilog module and file.

Generated NAND registers use the project main clock. Additional clocks of an
external hardware block are ordinary input ports; its emulator is responsible
for detecting and applying those clock edges.

Reusable, target-independent hardware lives under `components` and is also
re-exported at the crate root. For example,
`ClockDivider<DIVISOR, WIDTH>` produces a registered one-cycle clock-enable
pulse and supports emulation, NAND construction, and hierarchical Verilog.
Its `ClockDividerState` lets a larger module emulator reuse the same cycle
semantics. It intentionally does not generate a derived clock, so downstream
logic remains in the project's main clock domain. These small components are
part of the hardware crate without a feature flag: they add no optional
runtime or toolchain dependency. The public root re-export also leaves room to
move their implementation into another crate later without changing designs.

Generic hardware uses a structured identity rather than deriving HDL names
from Rust source paths:

```rust
#[derive(Hardware)]
#[hardware(namespace = "components/timing")]
pub struct ClockDivider<const DIVISOR: u64, const WIDTH: usize>;
```

`ClockDivider<3, 2>` therefore exports module
`ClockDivider_DIVISOR3_WIDTH2` to
`components/timing/clock_divider/divisor3_width2.v`. Each concrete
specialization has one definition file. Reusing that type creates additional
instances but does not duplicate its definition; a different specialization
gets a different module and file. Type and lifetime generics are rejected by
the derive and need concrete hardware wrapper types. No Verilog parameters are
generated. Every generated file starts with comments containing the original
Rust type and logical Verilog path.

Hierarchical modules can share one structural implementation while choosing
child implementations explicitly:

```rust
fn build(
    input: &Input,
    divider: fn(&ClockDividerInput) -> ClockDividerOutput,
) -> Output {
    let tick = divider(&ClockDividerInput {});
    // Build the remaining structure using tick.tick.
}

fn nand(input: &Input) -> Output {
    build(input, ClockDivider::<DIVISOR, 23>::nand)
}

fn build_verilog(input: &Input) -> Output {
    build(input, ClockDivider::<DIVISOR, 23>::verilog)
}
```

Host construction may mix `Child::emu` and `Child::nand` deliberately. Hardware
export always enters through `build_verilog`: a child's `verilog` uses its
verified hand-written implementation when present and otherwise recursively
exports the child's NAND implementation.

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

## Hand-written Verilog verification

Hand-written Verilog uses the same `ModuleTest` steps and expected values as
emu and NAND. The framework generates the HDL testbench from those vectors, so
there is no second copy of test data. `TestStep::after_cycles(N)` represents a
compact run of main-clock edges before sampling, including large dividers.
Both source and generated testbench text are included in the checked-in
verification hash. Normal `cargo test` never launches an external HDL tool;
simulation tests are marked `#[ignore]` and must be requested explicitly:

```text
cargo test -p digital-design-hardware --all-targets verify_handwritten_verilog_with_iverilog -- --ignored --nocapture
```

Install Icarus Verilog first, or set `IVERILOG` and `VVP` to the executable
paths. A successful testbench must print `DIGITAL_DESIGN_PASS`. The helper then
prints a `ModuleName=fnv1a64:...` line; copy that exact line to the module's
`.verified` file. Generation rejects missing or stale records without exposing
the replacement hash; only a successful explicit simulation prints it. The
manifest is an auditable attestation, not a substitute for the simulation run
that produced it.

## Hardware targets and resources

A project selects one complete, purchasable hardware variant as a Rust type.
The target directly combines its backend, exact Gowin selection fields,
fabric capacity, fitted memory, and board-level components:

```rust
type Target = TangNano20K;
let project = Target::user_io_project::<CounterBoard>("counter");
```

There is deliberately no separate FPGA-model type: repeating chip capacities
for a second board revision is simpler and less ambiguous than merging device,
package, stepping, memory-fit, and dock identities. A target supports a
component when its inventory contains every lower-level resource required by
that component. Resource requests belong only to target bindings and leaf
modules that directly represent physical components:

```rust
#[derive(Hardware)]
#[hardware(namespace = "components/memory", target_leaf)]
struct FrameBuffers;

impl Module for FrameBuffers {
    // Other associated items omitted.

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![
            TargetResourceRequest::new(BsramBlocks::new(4)),
            TargetResourceRequest::new(DspMultipliers::new(2)),
        ]
    }
}
```

The explicit `target_leaf` marker is the capability to claim target resources.
An upper module obtains them only by instantiating the leaf. Export rejects a
resource-owning module without the marker or with children, and counts
each actual leaf instance even when its Verilog definition is deduplicated.
Board clocks, buttons, and LEDs are reserved by the board binding for the same
reason. Capacity overflow panics during Gowin project generation with the
target, full instance path, Rust type, component, requested quantity, and
remaining capacity; generation does not continue.

Every `TargetComponent` returns a list of lower-level `ResourceAmount`
requirements. A take is transactional across the whole list. BSRAM reserves
18-Kbit blocks and DSP reserves 18x18 multiplier capacity, which are common to
the currently modeled Gowin variants. SDR SDRAM and DDR3 remain different
resource kinds. Fitted SDR SDRAM, DDR3, and SPI flash are indivisible devices:
a target exposes one complete device, and a second claim exceeds capacity.
Their physical bit capacities remain target metadata and appear in the report;
they are not allocatable bit balances. SSRAM remains a divisible FPGA fabric
resource. Later BSRAM/DSP libraries can add configuration variants whose
measured block consumption is declared by each concrete implementation.
The lower-level allocator remains transactional and is poisoned after a failed
claim. Exported Gowin projects include `resource-report.txt`; synthesis and
place-and-route remain the final authority.

Tang Nano 20K exposes the wires that are stable parts of the board directly as
typed module IO:

```rust
impl Module for CounterBoard {
    type Input = TangNano20KInputs;
    type Output = TangNano20KOutputs;

    fn nand(input: &Self::Input) -> Self::Output {
        let count = Counter::nand(&CounterInput {
            reset: input.buttons.wires[0],
            enable: input.buttons.wires[1],
        });
        TangNano20KOutputs { leds: count.value }
    }
}

let project = TangNano20K::user_io_project::<CounterBoard>("counter");
```

The project factory automatically reserves the clock, buttons, and LEDs and
generates the board wrapper, CST, and SDC files. The top module type is fixed
in `GowinModuleProject`, so export cannot substitute a module with different
IO. Physical port names remain private target implementation details. Board
bindings reject two logical signals assigned to the same physical pin,
including collisions with the clock.

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

Bidirectional IO is not exported yet. `InOutSignals` defines the internal
read/write/write-enable contract for a future target IO-buffer leaf; the
remaining resolution and binding work is recorded in `INOUT.md`.

## Basic adder Gowin example

Export the Tang Nano 20K example:

```text
cargo run -p digital-design-hardware --example basic_adder -- target/basic_adder_gowin
```

The result contains generated hierarchical HDL, a board wrapper, CST, SDC,
Gowin project XML, and `build.tcl`. The testable `BasicAdderBoard` hardware
module synchronizes both buttons and produces a 4 Hz enable from the 27 MHz
clock. The generated binding connects it to pin 4, the two buttons, and the six
active-low LEDs. No hand-written board directory is used.

Build without programming the board:

```powershell
cargo run -p digital-design-hardware --example basic_adder -- --build
```

Gowin tool discovery uses an explicit `--gowin-home PATH` first, then the
`GOWIN_HOME` environment variable, then `gw_sh`/`programmer_cli` on `PATH`.
For example, the current machine can use:

```powershell
$env:GOWIN_HOME = 'D:\DevTools\Gowin\Gowin_V1.9.11.03_Education_x64'
cargo run -p digital-design-hardware --example basic_adder -- --build
```

Or without changing the environment:

```powershell
cargo run -p digital-design-hardware --example basic_adder -- --build --gowin-home 'D:\DevTools\Gowin\Gowin_V1.9.11.03_Education_x64'
```

SRAM programming is intentionally an explicit operation and is never run by
`cargo test`. With a connected board it can be requested with `--program`;
this first performs a clean export and build, then programs volatile SRAM.
