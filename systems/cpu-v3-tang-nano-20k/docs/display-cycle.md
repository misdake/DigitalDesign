# Display cycle components

The `hardware::display_cycle` module provides an edge-driven Rust mirror of the production
`DisplayLineBuffer` leaf. It owns no root clock, backing memory, MC, or device callbacks.
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
