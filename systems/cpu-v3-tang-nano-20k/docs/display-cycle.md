# Display cycle components

The `hardware::display_cycle` module provides edge-driven Rust mirrors of the production
`DisplayLineBuffer` and the logic-domain fill/Device3 controller. It owns no root clock,
backing memory, MC, or device callbacks.
The existing functional Display and Verilog-only hardware wrapper retain their behavior.

## Line RAM

`LineBuffer::clock(Edges, &LineBufferInput)` commits one scheduler instant. Logic rises write
both 32-bit banks from one 64-bit beat; pixel rises register independent read addresses.
Coincident reads observe the old array before the write. Addresses truncate to nine bits.
With no pixel rise, Q holds even if addresses or storage change. Repeated `outputs()` and
passive `peek()` calls have no side effects.

There are two 512-word banks. Q starts at zero. Only cells 416..511 are initialized from the
same authoritative precode packing functions and linear-color table used by the RTL exporter.
Data cells 0..399 and unused cells 400..415 start unknown in this behavioral model. The numeric
placeholder in an unknown `RamWord` is not a defined read value. There is no RAM reset port;
a Display reset does not clear storage or regenerate the tables.

`RamWord::known` records behavioral definedness. `collision` separately marks a same-address
write/read on coincident edges in that bank; its numeric value still describes behavioral
old-array RTL semantics. The flag does not promise a silicon read-during-write value.
A valid consumer must independently exclude unknown or collision-tainted effective operands.
This leaf does not model pixel consumption or make that whole-Display claim.

## Bounded qualification

`tests/display_cycle_ram.rs` compares each registered read against the production
`DisplayLineBuffer::verilog_source()`, including unknown values, all table addresses, Q holds,
two relative clock phases, and simultaneous read/write edges. Test copies with a wrong bank
write and a combinational read replace the real DUT behavior; both must fail at a RAM-Q oracle.
Each external process has a 30-second watchdog and the fixture has a 4096-row cap.
Commands, observed outputs, coverage, and Git object hashes are written under
`target/display-cycle-ram`.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label display-cycle-ram -CargoArgs @(
  '-p','cpu-v3-tang-nano-20k','--test','display_cycle_ram',
  '--','--include-ignored','--nocapture','--test-threads=1')
```

This qualifies behavioral RAM port ordering. It does not qualify the complete Display,
physical CDC/metastability, collision consumption safety, TMDS, or system memory competition.

## Logic-domain fill and Device3

`Fill::outputs(&FillInput)` reads old state without side effects;
`Fill::clock(&FillInput)` commits exactly one logic rise. The root must pre-sample
MC, device and pixel feedback before committing any coincident owner. Pixel feedback
is `released`, `frame_toggle`, `pixel_underflow` and `pixel_started`; this leaf
does not advance that consumer or an MC.

Outputs include the fixed read request/address, direct 64-bit line-RAM write,
publication/format/valid metadata, Device3 read data and health/start/frame events.
Addresses are 22-bit 16-bit-word addresses. One request is 32 bytes/four 64-bit beats.
The root ties Display response-ready high as in the production composition.
Before the first applied NEXT_SWAP, two-row groups are virtual black markers with no
DRAM requests or data writes. A group is reused only after release crosses two logic
synchronizer stages. A complete real two-row group requires 50 segments.

Fill uses the production Column16 tile/pair traversal, exact LAST/error handling,
sticky error state, frame-complete gate, shadow descriptor checks and staged format.
Its address limit is `0x3e7000`, based on the physical plane span rather than only
the live-pixel payload. Same-edge frame handling applies the old pending descriptor;
a new MMIO NEXT_SWAP remains pending for the next eligible frame event.
Runtime reset preserves the unreset release/started synchronizers and RAM storage,
matching the RTL; it clears the logic registers that have explicit reset assignments.

`tests/display_cycle_fill.rs` uses the unmodified production pixel timing/ownership
domain as a controlled consumer, with exact 54/33.3-MHz edge ratio and two pixel phases.
At each logic edge, before production NBA commits, it compares the boundary ports;
run-length encoding compresses the recording, never skips emulated edges.
All 400 data addresses in both RAM banks are checked at termination.
Cases cover a complete real frame, retained single group, exhausted prefills/underflow,
beat errors, malformed LAST, reset to black startup and terminal error without valid.
DUT copies shorten the release CDC, reverse frame/MMIO assignment priority, or ignore
memory error. A port divergence is required; a mutant timeout alone is not a detection.

Each process has a 90-second watchdog; each case has 4-frame/4000000-logic-edge limits
and the trace has a 150000-row cap. Sources, traces and exact commands remain under
`target/display-cycle-fill`.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label display-cycle-fill -CargoArgs @(
  '-p','cpu-v3-tang-nano-20k','--test','display_cycle_fill',
  '--','--include-ignored','--nocapture','--test-threads=1')
```

This qualifies the logic-domain leaf with a real RTL consumer and controlled memory
response peer. It does not qualify a Rust pixel consumer, color/TMDS conversion,
native MC competition, actual bootloader-end equivalence, or a complete system.