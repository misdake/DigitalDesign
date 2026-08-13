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

A Verilog-only board test harness sets `EMU_AVAILABLE` to false. Calling its
`emu` entry point then fails immediately; it must not provide placeholder
outputs that pretend to model the hardware test. Reusable hardware modules
continue to provide a real emulator.

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

When a handwritten Verilog module directly instantiates child modules, it
lists every physical instance through `Module::verilog_dependencies`. The
exporter emits each concrete child definition once, but applies its target-leaf
resource claim once per listed instance. Instance names are included in the
parent's verification hash, so changing that instance list requires another
successful HDL simulation. Generated structural modules do
not use this list because calls to `Child::verilog` already record instances.

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

## Explicit Verilog simulation

Hand-written Verilog uses the same `ModuleTest` steps and expected values as
emu and NAND. The framework generates the HDL testbench from those vectors, so
there is no second copy of test data. `TestStep::after_cycles(N)` represents a
compact run of main-clock edges before sampling, including large dividers.
Normal `cargo test` never launches an external HDL tool; simulation tests are
marked `#[ignore]` and must be requested explicitly. For example:

```text
cargo test -p digital-design-hardware --lib components::bsram::tests::verify_verilog_with_iverilog -- --ignored
```

Install Icarus Verilog first, or set `IVERILOG` and `VVP` to the executable
paths. A successful testbench must print `DIGITAL_DESIGN_PASS`. The helper then
returns success. There is deliberately no checked-in source hash or export-time
attestation: explicit HDL simulation and hardware validation remain available
without making every Verilog edit participate in a manifest workflow.

Every module that supplies `verilog_source` or `generated_verilog_source` must
also supply a `verilog_testbench`; export rejects explicit HDL without one.
Modules exported mechanically from `nand` need no separate Verilog testbench:
their module tests should instead run the same vectors against emulation and
NAND, while project-level export and synthesis tests cover the serializer.
Parameterized HDL should simulate small representative specializations. Large
real-world constants are covered by source/export assertions instead of
advancing millions of simulator cycles unless their timing behavior differs.

### BSRAM leaves

`Bsram1Rw1024<WIDTH, Image>`, `Bsram1R1Rw1024<WIDTH, Image>`, and
`BsramTrueDualPort1024<WIDTH, Image>` provide the initial 1024-word BSRAM shapes.
`WIDTH` must be 16 or 18; each concrete specialization claims one 18-Kbit
BSRAM block. These are target leaves with emulator and generated, explicitly
simulator-tested Verilog implementations. They intentionally have no NAND
implementation:
the FPGA memory primitive is their implementation boundary, and calling
`nand` fails immediately instead of expanding storage into gates.

All ports use the project clock and have synchronous registered reads. A
read/write port operates in normal mode: during a write its read output holds
the previous registered value. Every BSRAM requires an explicit `Image`; there
is no uninitialized variant, so emulation and FPGA startup have the same
contents. In the true-dual-port shape, simultaneous
writes to one address are unsupported and panic in emulation. Avoid depending
on same-address cross-port read/write collision values in portable modules;
they require a target- and configuration-specific measurement.

`Image` supplies one compile-time array shared by emulation and Verilog
generation:

```rust
use digital_design_hardware::{
    Bsram1Rw1024, BsramImage, BSRAM_1024_DEPTH,
};

const fn boot_words() -> [u64; BSRAM_1024_DEPTH] {
    let mut words = [0; BSRAM_1024_DEPTH];
    words[0] = 0x1234;
    words
}

struct BootImage;

impl BsramImage<16> for BootImage {
    const WORDS: [u64; BSRAM_1024_DEPTH] = boot_words();
}

type BootRam = Bsram1Rw1024<16, BootImage>;
```

Use `ZeroBsramImage` when all-zero startup is wanted; it remains an explicit
choice at the call site. The complete image is part of the concrete module
identity, so equal specializations are emitted once while different images
remain different modules. Gowin embeds the words in the volatile configuration
bitstream; they are present when the configured design starts and may
subsequently be overwritten normally. No runtime fill loop or external memory
file is required. All three port shapes use the same image mechanism.

Parameterized HDL templates live as complete, readable files beside their
Rust implementation (for example, `src/components/bsram/*.v`). Askama parses
those templates at Rust compile time and
binds their substitutions to typed Rust template structs. Const-generic Rust
specializations still render separate concrete Verilog modules; Verilog
parameters are not introduced. Keep HDL structure in the template and limit
Rust rendering code to values such as the concrete module name and bus width.

The explicit module simulation uses the same typed vectors as emulation:

```text
cargo test -p digital-design-hardware --lib components::bsram::tests::verify_verilog_with_iverilog -- --ignored --nocapture
```

The `bsram` example instantiates all three shapes at both widths with zero
images plus a writable RAM with a patterned image, and checks all 1024 startup
words before writing anything. It then confirms a write replaces the selected
patterned word. It also verifies on hardware that read/write outputs hold during
writes while the independent read-only ports continue updating. Build and
program volatile SRAM with:

```text
cargo run -p digital-design-hardware --example bsram -- --program
```

Its debug UART repeatedly sends checksummed `DDHT` status frames with BSRAM
test ID `0x01`. Capture raw bytes with the serial receiver, then use the shared
development script to verify the identity, freshness, checksum, and result:

```powershell
powershell -ExecutionPolicy Bypass -File hardware/scripts/check_uart_status.ps1 -Path target/bsram_gowin/board_capture.bin -TestId 0x01
```

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
Unsupported configurations fail while reporting resources rather than using
an estimate. The initial six 1024x16/18 BSRAM specializations have each been
measured as one block; instantiating the same specialization more than once
repeats that claim even though its Verilog definition is emitted only once.
The lower-level allocator remains transactional and is poisoned after a failed
claim. Exported Gowin projects include `resource-report.txt`; synthesis and
place-and-route remain the final authority. After synthesis, the build audits
Gowin's hierarchical resource report: every actual BSRAM must be inside the
instance hierarchy of a target-leaf wrapper, and its use may not exceed that
wrapper's measured claim. A wrapper optimized away still reserves its planning
capacity, but cannot hide BSRAM inferred elsewhere. PnR totals provide a second
check before programming. PLL and DSP claims fail closed while no measured
per-instance report mapping exists; each future configuration must add that
mapping before it can be enabled.

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
The Programmer cable index selects a cable-driver type, not a USB enumeration
position. Each target supplies its normal type automatically (`USB Debugger A`
for the current Tang targets), so moving to another development machine does
not require a numeric setting. `--cable-index N` remains available for an
unusual driver setup. With at most one connected cable of that type, Gowin
Programmer selects it directly.
